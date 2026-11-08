//! Live CPU, memory and disk figures, the process list with a verdict on each
//! app, ending processes, and freeing memory.

use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub struct Monitor {
    pub sys: Mutex<System>,
    /// Compressed-store size, read off-thread: it needs PowerShell, which is
    /// too slow for every live tick. 0 means unknown.
    pub compressed: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl Monitor {
    pub fn new() -> Self {
        let mut sys = System::new();
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind());
        let compressed = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        if cfg!(windows) {
            let c = std::sync::Arc::clone(&compressed);
            std::thread::spawn(move || loop {
                if let Some(n) = crate::sysinfo_dev::compressed_now() {
                    c.store(n, std::sync::atomic::Ordering::Relaxed);
                }
                std::thread::sleep(std::time::Duration::from_secs(15));
            });
        }
        Self { sys: Mutex::new(sys), compressed }
    }
}

fn kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing().with_cpu().with_memory().with_exe(sysinfo::UpdateKind::OnlyIfNotSet)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskStat {
    pub mount: String,
    pub total: u64,
    pub free: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiveStats {
    pub cpu: f32,
    pub cores: Vec<f32>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// Bytes held by the OS memory compressor, when it can be read.
    pub compressed: Option<u64>,
    pub disk: Option<DiskStat>,
    pub process_count: usize,
    pub uptime: u64,
}

pub fn live(m: &Monitor) -> LiveStats {
    let mut sys = m.sys.lock().unwrap();
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind());
    let compressed = Some(m.compressed.load(std::sync::atomic::Ordering::Relaxed)).filter(|n| *n > 0);
    LiveStats {
        cpu: sys.global_cpu_usage(),
        cores: sys.cpus().iter().map(|c| c.cpu_usage()).collect(),
        mem_total: sys.total_memory(),
        mem_used: sys.used_memory(),
        mem_available: sys.available_memory(),
        swap_total: sys.total_swap(),
        swap_used: sys.used_swap(),
        compressed,
        disk: system_disk(),
        process_count: sys.processes().len(),
        uptime: System::uptime(),
    }
}

pub fn system_disk() -> Option<DiskStat> {
    let disks = Disks::new_with_refreshed_list();
    let want = if cfg!(windows) {
        std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()) + "\\"
    } else {
        "/".into()
    };
    let pick = disks
        .list()
        .iter()
        .find(|d| d.mount_point().to_string_lossy().eq_ignore_ascii_case(&want))
        // macOS: the data volume holds user files.
        .or_else(|| disks.list().iter().find(|d| d.mount_point().to_string_lossy() == "/System/Volumes/Data"))
        .or_else(|| disks.list().first())?;
    Some(DiskStat {
        mount: pick.mount_point().to_string_lossy().trim_end_matches('\\').to_string(),
        total: pick.total_space(),
        free: pick.available_space(),
    })
}

// ---------------------------------------------------------------- processes

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Safe,
    SaveFirst,
    System,
    Unknown,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProcGroup {
    pub name: String,
    pub pids: Vec<u32>,
    pub memory: u64,
    /// Share of the whole machine, 0-100.
    pub cpu: f32,
    pub verdict: Verdict,
    pub reason: String,
    pub protected: bool,
    pub exe: Option<String>,
}

fn display_name(raw: &str) -> String {
    let n = raw.strip_suffix(".exe").or_else(|| raw.strip_suffix(".EXE")).unwrap_or(raw);
    n.to_string()
}

/// Inside a Flatpak, sysinfo sees only the sandbox; list the host's
/// processes with `ps` instead. CPU here is ps's lifetime average.
fn host_processes() -> Vec<ProcGroup> {
    let Some(out) = crate::util::run_capture(Path::new("ps"), &["-eo", "pid=,rss=,pcpu=,comm="], std::time::Duration::from_secs(8)) else {
        return vec![];
    };
    let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f32;
    let mut groups: HashMap<String, ProcGroup> = HashMap::new();
    for line in out.lines() {
        let mut it = line.split_whitespace();
        let (Some(pid), Some(rss), Some(cpu)) = (it.next(), it.next(), it.next()) else { continue };
        let name: String = it.collect::<Vec<_>>().join(" ");
        let (Ok(pid), Ok(rss), Ok(cpu)) = (pid.parse::<u32>(), rss.parse::<u64>(), cpu.parse::<f32>()) else { continue };
        if name.is_empty() {
            continue;
        }
        let g = groups.entry(name.to_ascii_lowercase()).or_insert_with(|| {
            let (verdict, reason) = classify(&name, pid);
            ProcGroup { protected: verdict == Verdict::System, name: name.clone(), pids: vec![], memory: 0, cpu: 0.0, verdict, reason: reason.into(), exe: None }
        });
        g.pids.push(pid);
        g.memory += rss * 1024;
        g.cpu += cpu / ncpu;
    }
    let mut out: Vec<ProcGroup> = groups.into_values().collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.memory));
    out
}

pub fn processes(m: &Monitor) -> Vec<ProcGroup> {
    if crate::util::in_flatpak() {
        return host_processes();
    }
    let mut sys = m.sys.lock().unwrap();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind());
    let ncpu = sys.cpus().len().max(1) as f32;
    let me = std::process::id();
    let mut groups: HashMap<String, ProcGroup> = HashMap::new();
    for (pid, p) in sys.processes() {
        let raw = p.name().to_string_lossy().to_string();
        if raw.is_empty() {
            continue;
        }
        let name = display_name(&raw);
        let g = groups.entry(name.to_ascii_lowercase()).or_insert_with(|| {
            let (verdict, reason) = classify(&name, pid.as_u32());
            ProcGroup {
                protected: verdict == Verdict::System,
                name: name.clone(),
                pids: vec![],
                memory: 0,
                cpu: 0.0,
                verdict,
                reason: reason.into(),
                exe: p.exe().map(|e| e.to_string_lossy().to_string()),
            }
        });
        g.pids.push(pid.as_u32());
        g.memory += p.memory();
        g.cpu += p.cpu_usage() / ncpu;
        if pid.as_u32() == me {
            g.verdict = Verdict::System;
            g.protected = true;
            g.reason = "This app".into();
        }
    }
    let mut out: Vec<ProcGroup> = groups.into_values().collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.memory));
    out
}

/// A verdict and a short reason for an app, by name. Conservative: anything
/// the OS needs is protected, anything that may hold unsaved work says so.
pub fn classify(name: &str, pid: u32) -> (Verdict, &'static str) {
    let n = name.to_ascii_lowercase();
    let n = n.as_str();
    if pid <= 4 {
        return (Verdict::System, "Core of the OS");
    }
    const SYSTEM: &[(&str, &str)] = &[
        ("system", "Core of the OS"), ("registry", "Core of the OS"), ("smss", "Session manager"),
        ("csrss", "Needed by Windows"), ("wininit", "Needed by Windows"), ("winlogon", "Handles sign-in"),
        ("services", "Runs Windows services"), ("lsass", "Security and sign-in"), ("svchost", "Hosts Windows services"),
        ("dwm", "Draws your screen"), ("fontdrvhost", "Font driver"), ("memory compression", "Compressed memory store"),
        ("msmpeng", "Windows Defender"), ("nissrv", "Windows Defender"), ("securityhealthservice", "Windows Security"),
        ("audiodg", "Windows audio"), ("spoolsv", "Print spooler"), ("sihost", "Shell infrastructure"),
        ("ctfmon", "Keyboard and input"), ("conhost", "Console host"), ("runtimebroker", "App permissions"),
        ("taskhostw", "Task host"), ("searchindexer", "Windows Search"), ("wudfhost", "Device drivers"),
        ("lsaiso", "Credential guard"), ("secure system", "Virtualization security"), ("idle", "Idle time"),
        ("system idle process", "Idle time"), ("kernel_task", "Core of macOS"), ("launchd", "Starts everything on macOS"),
        ("windowserver", "Draws your screen"), ("loginwindow", "Your login session"), ("mds", "Spotlight"),
        ("mds_stores", "Spotlight"), ("coreaudiod", "macOS audio"), ("logd", "System logging"),
        ("dev cleaner", "This app"), ("dev-cleaner", "This app"),
    ];
    const SAVE_FIRST: &[(&str, &str)] = &[
        ("code", "May have unsaved files"), ("cursor", "May have unsaved files"), ("windsurf", "May have unsaved files"),
        ("zed", "May have unsaved files"), ("devenv", "May have unsaved files"), ("idea64", "May have unsaved files"),
        ("idea", "May have unsaved files"), ("webstorm64", "May have unsaved files"), ("pycharm64", "May have unsaved files"),
        ("rustrover64", "May have unsaved files"), ("goland64", "May have unsaved files"), ("studio64", "May have unsaved files"),
        ("sublime_text", "May have unsaved files"), ("notepad++", "May have unsaved files"), ("notepad", "May have unsaved files"),
        ("xcode", "May have unsaved files"), ("winword", "May have unsaved documents"), ("excel", "May have unsaved documents"),
        ("powerpnt", "May have unsaved documents"), ("photoshop", "May have unsaved documents"), ("figma", "May have unsaved work"),
        ("obs64", "Stops any recording"), ("docker desktop", "Stops running containers"), ("com.docker.backend", "Stops running containers"),
        ("vmmem", "Shuts down WSL and Docker"), ("vmmemwsl", "Shuts down WSL"), ("postgres", "Drops open connections"),
        ("mysqld", "Drops open connections"), ("mongod", "Drops open connections"), ("redis-server", "Drops open connections"),
        ("explorer", "Restarts the taskbar and desktop"), ("finder", "Relaunches by itself"), ("outlook", "May have unsent mail"),
        ("ms-teams", "Leaves any call"), ("teams", "Leaves any call"), ("zoom", "Leaves any call"),
        ("windowsterminal", "Closes your terminal tabs"), ("wezterm-gui", "Closes your terminal tabs"),
        ("iterm2", "Closes your terminal tabs"), ("terminal", "Closes your terminal tabs"), ("alacritty", "Closes your terminal tabs"),
        ("claude", "Ends the agent session"), ("codex", "Ends the agent session"),
    ];
    const SAFE: &[(&str, &str)] = &[
        ("chrome", "Tabs restore on relaunch"), ("msedge", "Tabs restore on relaunch"), ("firefox", "Tabs restore on relaunch"),
        ("brave", "Tabs restore on relaunch"), ("arc", "Tabs restore on relaunch"), ("opera", "Tabs restore on relaunch"),
        ("vivaldi", "Tabs restore on relaunch"), ("safari", "Tabs restore on relaunch"), ("google chrome", "Tabs restore on relaunch"),
        ("google chrome helper", "Part of Chrome"), ("msedgewebview2", "Embedded browser view"),
        ("node", "Dev server or tool, restart it"), ("bun", "Dev server or tool, restart it"), ("deno", "Dev server or tool, restart it"),
        ("python", "Script or server, restart it"), ("python3", "Script or server, restart it"), ("java", "JVM or Gradle daemon"),
        ("rust-analyzer", "Your editor restarts it"), ("tsserver", "Your editor restarts it"), ("esbuild", "Build helper"),
        ("cargo", "Build in progress stops"), ("rustc", "Build in progress stops"), ("vbcscompiler", "Build server, restarts on demand"),
        ("msbuild", "Build server, restarts on demand"), ("gopls", "Your editor restarts it"), ("pyright", "Your editor restarts it"),
        ("slack", "Reopens where you left off"), ("discord", "Reopens where you left off"), ("spotify", "Playback stops"),
        ("whatsapp", "Reopens where you left off"), ("telegram", "Reopens where you left off"), ("steam", "Restarts on demand"),
        ("steamwebhelper", "Part of Steam"), ("onedrive", "Sync resumes on relaunch"), ("dropbox", "Sync resumes on relaunch"),
        ("phoneexperiencehost", "Phone Link, reopens on demand"), ("widgets", "Reopens on demand"), ("searchhost", "Reopens on demand"),
        ("gitkraken", "Reopens where you left off"), ("postman", "Reopens where you left off"),
    ];
    for (list, v) in [(SYSTEM, Verdict::System), (SAVE_FIRST, Verdict::SaveFirst), (SAFE, Verdict::Safe)] {
        if let Some((_, why)) = list.iter().find(|(k, _)| *k == n) {
            return (v, why);
        }
    }
    if n.ends_with("helper") || n.contains("helper (renderer)") || n.contains("crashpad") {
        return (Verdict::Safe, "Helper, restarts with its app");
    }
    (Verdict::Unknown, "Not recognised, check before ending")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KillResult {
    pub ended: u32,
    pub failed: u32,
    pub freed: u64,
}

/// End the given processes, refusing anything protected.
/// Inside a Flatpak, end host processes with the host's `kill`.
fn host_kill(pids: &[u32]) -> KillResult {
    let groups = host_processes();
    let mut res = KillResult { ended: 0, failed: 0, freed: 0 };
    for &pid in pids {
        let Some(g) = groups.iter().find(|g| g.pids.contains(&pid)) else { continue };
        if g.protected {
            res.failed += 1;
            continue;
        }
        let ok = crate::util::run_ok(Path::new("kill"), &["-TERM", &pid.to_string()], std::time::Duration::from_secs(5));
        if ok {
            res.ended += 1;
            res.freed += g.memory / g.pids.len().max(1) as u64;
        } else {
            res.failed += 1;
        }
    }
    res
}

pub fn kill(m: &Monitor, pids: &[u32]) -> KillResult {
    if crate::util::in_flatpak() {
        return host_kill(pids);
    }
    let mut sys = m.sys.lock().unwrap();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, kind());
    let me = std::process::id();
    let mut res = KillResult { ended: 0, failed: 0, freed: 0 };
    for &pid in pids {
        let Some(p) = sys.process(Pid::from_u32(pid)) else { continue };
        let name = display_name(&p.name().to_string_lossy());
        if pid == me || classify(&name, pid).0 == Verdict::System {
            res.failed += 1;
            continue;
        }
        let mem = p.memory();
        if p.kill() {
            res.ended += 1;
            res.freed += mem;
        } else {
            res.failed += 1;
        }
    }
    res
}

// ------------------------------------------------------------- free memory

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeResult {
    pub before: u64,
    pub after: u64,
    pub trimmed: u32,
    pub note: String,
}

pub fn free_memory(m: &Monitor) -> FreeResult {
    let before = {
        let mut sys = m.sys.lock().unwrap();
        sys.refresh_memory();
        sys.used_memory()
    };
    let (trimmed, note) = trim();
    std::thread::sleep(std::time::Duration::from_millis(600));
    let after = {
        let mut sys = m.sys.lock().unwrap();
        sys.refresh_memory();
        sys.used_memory()
    };
    FreeResult { before, after, trimmed, note }
}

/// Windows: ask every process we may open to give back idle pages. Nothing is
/// closed; pages come back from the standby list as soon as they are touched.
#[cfg(windows)]
fn trim() -> (u32, String) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut n = 0;
    for pid in sys.processes().keys() {
        // SAFETY: plain Win32 calls on handles we open and close here.
        unsafe {
            let h = OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid.as_u32());
            if h.is_null() {
                continue;
            }
            if K32EmptyWorkingSet(h) != 0 {
                n += 1;
            }
            CloseHandle(h);
        }
    }
    let note = if crate::sysinfo_dev::is_admin() {
        "Trimmed idle memory from every app."
    } else {
        "Trimmed idle memory from your apps. Run as admin to include services."
    };
    (n, note.into())
}

/// macOS: `purge` flushes the disk cache. It needs an admin password.
#[cfg(target_os = "macos")]
fn trim() -> (u32, String) {
    let ok = crate::util::run_ok(
        std::path::Path::new("/usr/bin/osascript"),
        &["-e", "do shell script \"/usr/sbin/purge\" with administrator privileges"],
        std::time::Duration::from_secs(60),
    );
    if ok {
        (1, "Flushed inactive memory with purge.".into())
    } else {
        (0, "purge was cancelled or is unavailable.".into())
    }
}

/// Linux: drop the page cache. Needs root, asked for through pkexec.
#[cfg(target_os = "linux")]
fn trim() -> (u32, String) {
    let ok = crate::util::which("pkexec").is_some_and(|pk| {
        crate::util::run_ok(&pk, &["sh", "-c", "sync; echo 1 > /proc/sys/vm/drop_caches"], std::time::Duration::from_secs(60))
    });
    if ok {
        (1, "Dropped the page cache.".into())
    } else {
        (0, "Cancelled, or pkexec is not available.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_processes_are_protected() {
        assert_eq!(classify("csrss", 600).0, Verdict::System);
        assert_eq!(classify("System", 4).0, Verdict::System);
        assert_eq!(classify("WindowServer", 300).0, Verdict::System);
    }

    #[test]
    fn editors_ask_to_save_first_and_browsers_are_safe() {
        assert_eq!(classify("Code", 9000).0, Verdict::SaveFirst);
        assert_eq!(classify("chrome", 9000).0, Verdict::Safe);
        assert_eq!(classify("some-random-tool", 9000).0, Verdict::Unknown);
    }
}
