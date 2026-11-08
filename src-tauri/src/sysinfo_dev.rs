//! Facts about the machine a developer cares about: OS, CPU, memory and
//! compression, page file, GPUs, WSL, and which runtimes, editors and AI
//! coding CLIs are installed.

use crate::util::{self, home, run_capture, which};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;
use sysinfo::System;

const T: Duration = Duration::from_secs(6);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub version: Option<String>,
    pub path: Option<String>,
    pub installed: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WslDistro {
    pub name: String,
    pub state: String,
    pub version: String,
    pub default: bool,
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PageFile {
    pub path: String,
    pub size: u64,
    pub used: u64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    pub mount: String,
    pub name: String,
    pub fs: String,
    pub kind: String,
    pub total: u64,
    pub free: u64,
    pub removable: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Battery {
    pub percent: f32,
    pub state: String,
    /// Full-charge capacity as a share of design capacity, 0-100.
    pub health: f32,
    pub cycles: Option<u32>,
    pub time_to_empty_mins: Option<u64>,
    pub time_to_full_mins: Option<u64>,
    pub vendor: Option<String>,
    pub model: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os_name: String,
    pub os_version: String,
    pub kernel: String,
    pub hostname: String,
    pub arch: String,
    pub uptime: u64,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    pub cpu_freq_mhz: u64,
    pub mem_total: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub compression: Option<bool>,
    pub compressed_bytes: Option<u64>,
    pub page_files: Vec<PageFile>,
    pub gpus: Vec<String>,
    /// Memory held by the WSL / Docker virtual machine (vmmem), Windows only.
    pub vm_memory: Option<u64>,
    pub power_plan: Option<String>,
    /// Whether any ReFS (Dev Drive) volume exists. Windows only.
    pub dev_drive: Option<bool>,
    pub drives: Vec<Drive>,
    pub battery: Option<Battery>,
    pub developer_mode: Option<bool>,
    pub long_paths: Option<bool>,
    pub is_admin: bool,
    pub wsl_version: Option<String>,
    pub wsl: Vec<WslDistro>,
    pub runtimes: Vec<Tool>,
    pub editors: Vec<Tool>,
    pub ai_tools: Vec<Tool>,
}

pub fn collect() -> SystemInfo {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let cpus = sys.cpus();

    // The slow probes run side by side.
    let ((runtimes, editors), (ai_tools, (wsl, (probe, mac_gpus)))) = rayon::join(
        || rayon::join(runtimes, editors),
        || rayon::join(ai_tools, || rayon::join(wsl, || rayon::join(windows_probe, mac_gpus))),
    );
    let gpus = if cfg!(windows) { probe.gpus.clone() } else { mac_gpus };

    SystemInfo {
        os_name: System::name().unwrap_or_default(),
        os_version: System::long_os_version().unwrap_or_default(),
        kernel: System::kernel_version().unwrap_or_default(),
        hostname: System::host_name().unwrap_or_default(),
        arch: System::cpu_arch(),
        uptime: System::uptime(),
        cpu_brand: cpus.first().map(|c| c.brand().trim().to_string()).unwrap_or_default(),
        cpu_cores: System::physical_core_count().unwrap_or(cpus.len()),
        cpu_threads: cpus.len(),
        cpu_freq_mhz: cpus.first().map(|c| c.frequency()).unwrap_or(0),
        mem_total: sys.total_memory(),
        swap_total: sys.total_swap(),
        swap_used: sys.used_swap(),
        compression: compression(probe.compressed.is_some()),
        compressed_bytes: probe.compressed,
        page_files: probe.page_files,
        gpus,
        vm_memory: probe.vm_memory,
        power_plan: probe.power_plan,
        dev_drive: probe.dev_drive,
        drives: drives(),
        battery: battery(),
        developer_mode: reg_flag(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock", "AllowDevelopmentWithoutDevLicense"),
        long_paths: reg_flag(r"HKLM\SYSTEM\CurrentControlSet\Control\FileSystem", "LongPathsEnabled"),
        is_admin: is_admin(),
        wsl_version: wsl.0,
        wsl: wsl.1,
        runtimes,
        editors,
        ai_tools,
    }
}

/// Windows runs the compressor inside a process named "Memory Compression",
/// which exists only while compression is on. Reading it this way needs no
/// admin rights (Get-MMAgent does). macOS compresses memory always.
fn compression(win_process_found: bool) -> Option<bool> {
    if cfg!(windows) {
        Some(win_process_found)
    } else if cfg!(target_os = "macos") {
        let out = run_capture(Path::new("/usr/sbin/sysctl"), &["-n", "vm.compressor_mode"], T)?;
        Some(out.trim().parse::<u32>().is_ok_and(|m| m >= 2))
    } else {
        None
    }
}

fn probe(name: &str, cmds: &[&str], args: &[&str]) -> Tool {
    for c in cmds {
        if let Some(path) = which(c) {
            let out = run_capture(&path, args, T);
            let version = out.as_deref().and_then(|o| o.lines().find_map(util::version_token));
            return Tool { name: name.into(), version, path: Some(path.to_string_lossy().to_string()), installed: true };
        }
    }
    Tool { name: name.into(), version: None, path: None, installed: false }
}

fn runtimes() -> Vec<Tool> {
    let python: &[&str] = if cfg!(windows) { &["python", "py"] } else { &["python3", "python"] };
    let list: Vec<(&str, Vec<&str>, Vec<&str>)> = vec![
        ("Node.js", vec!["node"], vec!["--version"]),
        ("npm", vec!["npm"], vec!["--version"]),
        ("pnpm", vec!["pnpm"], vec!["--version"]),
        ("Yarn", vec!["yarn"], vec!["--version"]),
        ("Bun", vec!["bun"], vec!["--version"]),
        ("Deno", vec!["deno"], vec!["--version"]),
        ("Python", python.to_vec(), vec!["--version"]),
        ("uv", vec!["uv"], vec!["--version"]),
        ("Rust", vec!["rustc"], vec!["--version"]),
        ("Cargo", vec!["cargo"], vec!["--version"]),
        ("Go", vec!["go"], vec!["version"]),
        ("Java", vec!["java"], vec!["-version"]),
        (".NET", vec!["dotnet"], vec!["--version"]),
        ("Ruby", vec!["ruby"], vec!["--version"]),
        ("PHP", vec!["php"], vec!["--version"]),
        ("Docker", vec!["docker"], vec!["--version"]),
        ("Git", vec!["git"], vec!["--version"]),
        ("GitHub CLI", vec!["gh"], vec!["--version"]),
    ];
    list.par_iter().map(|(n, c, a)| probe(n, c, a)).collect()
}

/// AI coding CLIs and agent harnesses, found on PATH.
fn ai_tools() -> Vec<Tool> {
    let list: Vec<(&str, &str)> = vec![
        ("Claude Code", "claude"),
        ("OpenAI Codex", "codex"),
        ("Gemini CLI", "gemini"),
        ("GitHub Copilot CLI", "copilot"),
        ("Cursor Agent", "cursor-agent"),
        ("Aider", "aider"),
        ("OpenCode", "opencode"),
        ("Amp", "amp"),
        ("Goose", "goose"),
        ("Qwen Code", "qwen"),
        ("Crush", "crush"),
        ("Factory Droid", "droid"),
        ("Kiro CLI", "kiro-cli"),
        ("Ollama", "ollama"),
    ];
    let mut v: Vec<Tool> = list.par_iter().map(|(n, c)| probe(n, &[c], &["--version"])).collect();
    v.sort_by_key(|t| !t.installed);
    v
}

/// Editors and IDEs: a CLI on PATH gives a version; otherwise the usual
/// install folders say whether it is there.
fn editors() -> Vec<Tool> {
    let h = home();
    let local = dirs::data_local_dir().unwrap_or_else(|| h.clone());
    let pf = PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()));
    let apps = PathBuf::from("/Applications");
    let win = cfg!(windows);
    // (name, cli, install markers)
    let list: Vec<(&str, Option<&str>, Vec<PathBuf>)> = vec![
        ("VS Code", Some("code"), if win { vec![local.join("Programs/Microsoft VS Code"), pf.join("Microsoft VS Code")] } else { vec![apps.join("Visual Studio Code.app")] }),
        ("Cursor", Some("cursor"), if win { vec![local.join("Programs/cursor")] } else { vec![apps.join("Cursor.app")] }),
        ("Windsurf", Some("windsurf"), if win { vec![local.join("Programs/Windsurf")] } else { vec![apps.join("Windsurf.app")] }),
        ("Zed", Some("zed"), if win { vec![local.join("Programs/Zed")] } else { vec![apps.join("Zed.app")] }),
        ("Sublime Text", Some("subl"), if win { vec![pf.join("Sublime Text")] } else { vec![apps.join("Sublime Text.app")] }),
        ("Neovim", Some("nvim"), vec![]),
        ("Visual Studio", None, if win { vec![pf.join("Microsoft Visual Studio")] } else { vec![] }),
        ("JetBrains IDEs", None, if win { vec![pf.join("JetBrains"), local.join("JetBrains/Toolbox")] } else { vec![apps.join("IntelliJ IDEA.app"), apps.join("WebStorm.app"), apps.join("PyCharm.app"), apps.join("RustRover.app"), h.join("Library/Application Support/JetBrains/Toolbox")] }),
        ("Android Studio", None, if win { vec![pf.join("Android/Android Studio")] } else { vec![apps.join("Android Studio.app")] }),
        ("Xcode", None, if win { vec![] } else { vec![apps.join("Xcode.app")] }),
        ("Notepad++", None, if win { vec![pf.join("Notepad++")] } else { vec![] }),
    ];
    list.into_par_iter()
        .filter(|(_, cli, marks)| cli.is_some() || !marks.is_empty())
        .map(|(name, cli, marks)| {
            if let Some(c) = cli {
                let t = probe(name, &[c], &["--version"]);
                if t.installed {
                    return t;
                }
            }
            let hit = marks.iter().find(|m| m.exists());
            Tool { name: name.into(), version: None, path: hit.map(|p| p.to_string_lossy().to_string()), installed: hit.is_some() }
        })
        .collect()
}

fn wsl() -> (Option<String>, Vec<WslDistro>) {
    if !cfg!(windows) {
        return (None, vec![]);
    }
    let Some(exe) = which("wsl") else { return (None, vec![]) };
    let version = run_capture(&exe, &["--version"], T).and_then(|o| o.lines().next().and_then(util::version_token));
    let list = run_capture(&exe, &["-l", "-v"], T).unwrap_or_default();
    let distros = list
        .lines()
        .skip(1)
        .filter_map(|l| {
            let default = l.trim_start().starts_with('*');
            let parts: Vec<&str> = l.trim_start_matches([' ', '*']).split_whitespace().collect();
            (parts.len() >= 3).then(|| WslDistro {
                name: parts[0].into(),
                state: parts[1].into(),
                version: parts[2].into(),
                default,
            })
        })
        .collect();
    (version, distros)
}

fn powershell(script: &str) -> Option<String> {
    let ps = which("powershell")?;
    run_capture(&ps, &["-NoProfile", "-NonInteractive", "-Command", script], Duration::from_secs(10))
}

fn mac_gpus() -> Vec<String> {
    if !cfg!(target_os = "macos") {
        return vec![];
    }
    run_capture(Path::new("/usr/sbin/system_profiler"), &["SPDisplaysDataType"], Duration::from_secs(10))
        .map(|o| o.lines().filter_map(|l| l.trim().strip_prefix("Chipset Model:").map(|s| s.trim().to_string())).collect())
        .unwrap_or_default()
}

/// Everything Windows-only that needs WMI or protected-process figures, in
/// one PowerShell call. Get-Process reads the working set of protected
/// processes (Memory Compression, vmmem) without admin rights; sysinfo
/// cannot, and reported 0 B.
#[derive(Default, Debug)]
pub struct WinProbe {
    pub compressed: Option<u64>,
    pub vm_memory: Option<u64>,
    pub power_plan: Option<String>,
    pub dev_drive: Option<bool>,
    pub page_files: Vec<PageFile>,
    pub gpus: Vec<String>,
}

const PROBE: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$mc = (Get-Process -Name 'Memory Compression').WorkingSet64
$vm = (Get-Process -Name 'vmmem','VmmemWSL' | Measure-Object WorkingSet64 -Sum).Sum
$pp = ((powercfg /getactivescheme) -replace '^.*\((.*)\)\s*$','$1')
$refs = @(Get-Volume | Where-Object FileSystem -eq 'ReFS').Count
$pf = @(Get-CimInstance Win32_PageFileUsage | ForEach-Object { @{ path = $_.Name; size = [int64]$_.AllocatedBaseSize; used = [int64]$_.CurrentUsage } })
$gpu = @((Get-CimInstance Win32_VideoController).Name)
@{ mc = $mc; vm = $vm; pp = "$pp".Trim(); refs = $refs; pf = $pf; gpu = $gpu } | ConvertTo-Json -Compress -Depth 4
"#;

pub fn windows_probe() -> WinProbe {
    if !cfg!(windows) {
        return WinProbe::default();
    }
    let Some(out) = powershell(PROBE) else { return WinProbe::default() };
    parse_probe(out.lines().last().unwrap_or(""))
}

fn parse_probe(json: &str) -> WinProbe {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return WinProbe::default();
    };
    let mb = 1024 * 1024;
    WinProbe {
        compressed: v["mc"].as_u64(),
        vm_memory: v["vm"].as_u64().filter(|n| *n > 0),
        power_plan: v["pp"].as_str().filter(|s| !s.is_empty()).map(String::from),
        dev_drive: v["refs"].as_u64().map(|n| n > 0),
        page_files: v["pf"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|p| PageFile {
                        path: p["path"].as_str().unwrap_or("").to_string(),
                        size: p["size"].as_u64().unwrap_or(0) * mb,
                        used: p["used"].as_u64().unwrap_or(0) * mb,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        gpus: v["gpu"].as_array().map(|a| a.iter().filter_map(|g| g.as_str().map(String::from)).collect()).unwrap_or_default(),
    }
}

/// Just the compressed-store size, for live stats.
pub fn compressed_now() -> Option<u64> {
    if !cfg!(windows) {
        return None;
    }
    powershell("(Get-Process -Name 'Memory Compression' -ErrorAction SilentlyContinue).WorkingSet64")?.trim().parse().ok()
}

fn drives() -> Vec<Drive> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let mut v: Vec<Drive> = disks
        .list()
        .iter()
        .filter(|d| d.total_space() > 0)
        .filter(|d| {
            // macOS lists system snapshot volumes; keep the ones people use.
            let m = d.mount_point().to_string_lossy();
            !m.starts_with("/System/Volumes/") || m == "/System/Volumes/Data"
        })
        .map(|d| Drive {
            mount: d.mount_point().to_string_lossy().trim_end_matches('\\').to_string(),
            name: d.name().to_string_lossy().to_string(),
            fs: d.file_system().to_string_lossy().to_string(),
            kind: match d.kind() {
                sysinfo::DiskKind::SSD => "SSD".into(),
                sysinfo::DiskKind::HDD => "HDD".into(),
                _ => String::new(),
            },
            total: d.total_space(),
            free: d.available_space(),
            removable: d.is_removable(),
        })
        .collect();
    v.sort_by(|a, b| a.mount.cmp(&b.mount));
    v.dedup_by(|a, b| a.mount == b.mount);
    v
}

/// The first battery, or None on desktops. The Battery card only shows on
/// machines that have one.
fn battery() -> Option<Battery> {
    use starship_battery::units::ratio::percent;
    use starship_battery::units::time::minute;
    let manager = starship_battery::Manager::new().ok()?;
    let b = manager.batteries().ok()?.flatten().next()?;
    Some(Battery {
        percent: b.state_of_charge().get::<percent>(),
        state: format!("{:?}", b.state()).to_lowercase(),
        health: b.state_of_health().get::<percent>(),
        cycles: b.cycle_count(),
        time_to_empty_mins: b.time_to_empty().map(|t| t.get::<minute>() as u64),
        time_to_full_mins: b.time_to_full().map(|t| t.get::<minute>() as u64),
        vendor: b.vendor().map(String::from),
        model: b.model().map(String::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_windows_probe() {
        let p = parse_probe(r#"{"mc":5287034880,"vm":0,"pp":"High performance","refs":0,"pf":[{"path":"C:\\pagefile.sys","size":43627,"used":120}],"gpu":["AMD Radeon(TM) Graphics","NVIDIA GeForce RTX 5070"]}"#);
        assert_eq!(p.compressed, Some(5_287_034_880));
        assert_eq!(p.vm_memory, None);
        assert_eq!(p.power_plan.as_deref(), Some("High performance"));
        assert_eq!(p.dev_drive, Some(false));
        assert_eq!(p.page_files[0].size, 43627 * 1024 * 1024);
        assert_eq!(p.gpus.len(), 2);
    }

    #[test]
    fn a_failed_probe_is_empty_not_a_panic() {
        assert!(parse_probe("not json").compressed.is_none());
    }
}

/// A REG_DWORD that is 1 or 0. Not elevated; these keys are world-readable.
fn reg_flag(key: &str, value: &str) -> Option<bool> {
    if !cfg!(windows) {
        return None;
    }
    let reg = which("reg")?;
    let out = run_capture(&reg, &["query", key, "/v", value], T)?;
    let line = out.lines().find(|l| l.contains(value))?;
    let hex = line.split_whitespace().last()?;
    Some(u32::from_str_radix(hex.trim_start_matches("0x"), 16).ok()? != 0)
}

#[cfg(windows)]
pub fn is_admin() -> bool {
    // SAFETY: no arguments, returns a BOOL.
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

#[cfg(not(windows))]
pub fn is_admin() -> bool {
    std::env::var("USER").is_ok_and(|u| u == "root")
}

/// Start a second, elevated copy of the app. The caller exits on success.
pub fn relaunch_as_admin() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    if cfg!(windows) {
        let script = format!("Start-Process -FilePath '{}' -Verb RunAs", exe.to_string_lossy().replace('\'', "''"));
        which("powershell").is_some_and(|ps| util::run_ok(&ps, &["-NoProfile", "-Command", &script], Duration::from_secs(60)))
    } else {
        false
    }
}
