//! Orphaned dev processes, the WSL virtual machine and Docker containers.
//!
//! An orphan is a dev runtime (node, python, bun, java, ...) whose parent has
//! exited: the editor window, terminal or AI agent that started it is gone,
//! but it keeps its memory. On Windows that means the parent PID no longer
//! exists, or now belongs to a process that started later (the PID was
//! reused). On macOS and Linux an orphan is re-parented to PID 1 (launchd,
//! init) or a user systemd. Nothing with a live parent is listed, and
//! nothing is ended here: the frontend ends the ones the user picks through
//! the usual `kill_processes`, with its protections.

use crate::util::{self, which};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

type R<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

/// Run a program; (exited cleanly, stdout, stderr), or None when it is
/// missing or takes longer than `timeout`.
fn run_out(program: &Path, args: &[&str], timeout: Duration) -> Option<(bool, String, String)> {
    let child = util::command(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let (tx, rx) = mpsc::channel();
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => Some((out.status.success(), util::decode(&out.stdout), util::decode(&out.stderr))),
        _ => {
            let mut sys = sysinfo::System::new();
            let p = sysinfo::Pid::from_u32(pid);
            sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
            if let Some(proc_) = sys.process(p) {
                proc_.kill();
            }
            None
        }
    }
}

// ------------------------------------------------------------- orphans

const DEV: &[&str] = &[
    "node", "bun", "deno", "python", "python3", "pythonw", "py", "uv", "uvx", "java", "javaw", "dotnet", "go", "esbuild", "ruby", "php",
    "rust-analyzer", "gopls", "clangd", "tsserver", "npx", "pnpm", "yarn", "npm",
];

#[derive(Clone, Debug)]
pub(crate) struct ProcInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub start: u64,
    pub name: String,
    pub cmd: String,
    pub memory: u64,
}

fn base_name(name: &str) -> String {
    let n = name.to_ascii_lowercase();
    n.strip_suffix(".exe").unwrap_or(&n).to_string()
}

/// Why a process counts as orphaned, or None when its parent is alive.
pub(crate) fn orphan_reason(p: &ProcInfo, by_pid: &HashMap<u32, &ProcInfo>, unix: bool) -> Option<&'static str> {
    if !DEV.contains(&base_name(&p.name).as_str()) {
        return None;
    }
    let parent = p.parent?;
    if unix {
        if parent == 1 {
            return Some("Parent exited");
        }
        let pn = by_pid.get(&parent).map(|q| base_name(&q.name)).unwrap_or_default();
        // A user systemd adopts orphans too, but it also starts real
        // services, so only init and launchd count.
        return (pn == "launchd" || pn == "init").then_some("Parent exited");
    }
    match by_pid.get(&parent) {
        None => Some("Parent exited"),
        Some(q) if q.start > p.start => Some("Parent exited, its PID was reused"),
        Some(_) => None,
    }
}

/// Which family a command line belongs to: "mcp", "lsp", "dev" or "other".
pub(crate) fn group_of(cmd: &str) -> &'static str {
    let c = cmd.to_ascii_lowercase();
    let has = |xs: &[&str]| xs.iter().any(|x| c.contains(x));
    if has(&["mcp", "@modelcontextprotocol", "claude", "codex", "anthropic"]) {
        "mcp"
    } else if has(&[
        "tsserver", "typescript-language-server", "eslintserver", "eslint_d", "pyright", "pylance", "basedpyright", "rust-analyzer", "gopls",
        "jdtls", "language-server", "languageserver", "langserver", "volar", "vue-language", "tailwindcss-language", "omnisharp", "clangd",
        ".vscode", ".cursor", "server-node", "lsp",
    ]) {
        "lsp"
    } else if has(&[
        "vite", "next dev", "next-server", "next\\dist", "next/dist", "webpack", "nodemon", "tsx watch", "ts-node-dev", "uvicorn", "flask",
        "runserver", "astro", "nuxt", "remix", "turbo", "storybook", "vitest", "jest", "parcel", "rollup", "esbuild", "gunicorn", "--watch",
        "dev-server", "react-scripts", "expo", "metro",
    ]) {
        "dev"
    } else {
        "other"
    }
}

fn short_cmd(cmd: &str) -> String {
    let home = util::home().to_string_lossy().to_string();
    let mut s = if home.len() > 3 { cmd.replace(&home, "~") } else { cmd.to_string() };
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 200 {
        s = s.chars().take(199).collect::<String>() + "…";
    }
    s
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Orphan {
    pub pid: u32,
    pub name: String,
    /// "mcp", "lsp", "dev" or "other".
    pub group: String,
    pub memory: u64,
    /// Unix seconds.
    pub started_at: u64,
    pub cmd: String,
    pub why: String,
    pub verdict: crate::monitor::Verdict,
    pub reason: String,
    pub protected: bool,
}

pub(crate) fn find_orphans(procs: &[ProcInfo], unix: bool, me: u32) -> Vec<Orphan> {
    let by_pid: HashMap<u32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut out: Vec<Orphan> = procs
        .iter()
        .filter(|p| p.pid != me && p.pid > 4)
        .filter_map(|p| {
            let why = orphan_reason(p, &by_pid, unix)?;
            let name = p.name.strip_suffix(".exe").or_else(|| p.name.strip_suffix(".EXE")).unwrap_or(&p.name).to_string();
            let (verdict, reason) = crate::monitor::classify(&name, p.pid);
            Some(Orphan {
                pid: p.pid,
                group: group_of(&p.cmd).into(),
                memory: p.memory,
                started_at: p.start,
                cmd: short_cmd(if p.cmd.is_empty() { &p.name } else { &p.cmd }),
                why: why.into(),
                protected: verdict == crate::monitor::Verdict::System,
                verdict,
                reason: reason.into(),
                name,
            })
        })
        .collect();
    out.sort_by(|a, b| a.group.cmp(&b.group).then(b.memory.cmp(&a.memory)));
    out
}

fn read_procs() -> Vec<ProcInfo> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
    let mut sys = sysinfo::System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory().with_cmd(UpdateKind::Always));
    sys.processes()
        .iter()
        .map(|(pid, p)| ProcInfo {
            pid: pid.as_u32(),
            parent: p.parent().map(|x| x.as_u32()),
            start: p.start_time(),
            name: p.name().to_string_lossy().to_string(),
            cmd: p.cmd().iter().map(|a| a.to_string_lossy()).collect::<Vec<_>>().join(" "),
            memory: p.memory(),
        })
        .collect()
}

#[tauri::command]
pub async fn list_orphans() -> R<Vec<Orphan>> {
    if util::in_flatpak() {
        return Ok(vec![]);
    }
    blocking(|| find_orphans(&read_procs(), !cfg!(windows), std::process::id())).await
}

// ------------------------------------------------------------- WSL

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Distro {
    pub name: String,
    pub running: bool,
    pub version: String,
    pub default: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct LinuxMem {
    pub total: u64,
    /// What Linux programs hold: total minus free minus cache.
    pub used: u64,
    /// Page cache, buffers and reclaimable slab: Linux gives these back on demand.
    pub cache: u64,
    pub free: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct WslConfig {
    pub path: String,
    pub exists: bool,
    pub memory: Option<String>,
    pub processors: Option<String>,
    pub swap: Option<String>,
    pub auto_memory_reclaim: Option<String>,
    pub sparse_vhd: Option<String>,
    pub raw: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct WslStatus {
    /// WSL is installed (Windows only).
    pub available: bool,
    pub running: bool,
    pub distros: Vec<Distro>,
    /// Working set of the VM process (VmmemWSL / vmmem) as Windows sees it.
    pub vm_memory: Option<u64>,
    /// Inside the VM, read only when a distro is already running.
    pub linux: Option<LinuxMem>,
    pub config: WslConfig,
}

/// `[section] key=value` → lowercase "section.key" → value.
pub(crate) fn parse_ini(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut section = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some(s) = l.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = s.trim().to_ascii_lowercase();
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let v = v.split(['#']).next().unwrap_or("").trim().trim_matches('"').to_string();
            out.insert(format!("{section}.{}", k.trim().to_ascii_lowercase()), v);
        }
    }
    out
}

pub(crate) fn wslconfig_from(path: &Path, text: Option<String>) -> WslConfig {
    let raw = text.clone().unwrap_or_default();
    let ini = parse_ini(&raw);
    // Settings that left [experimental] are read from either section.
    let get = |k: &str| ini.get(&format!("wsl2.{k}")).or_else(|| ini.get(&format!("experimental.{k}"))).cloned();
    WslConfig {
        path: path.to_string_lossy().to_string(),
        exists: text.is_some(),
        memory: get("memory"),
        processors: get("processors"),
        swap: get("swap"),
        auto_memory_reclaim: get("automemoryreclaim"),
        sparse_vhd: get("sparsevhd"),
        raw,
    }
}

/// `wsl -l -v` (already decoded from UTF-16) → distros.
pub(crate) fn parse_wsl_list(text: &str, running: &[String]) -> Vec<Distro> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let default = l.trim_start().starts_with('*');
            let parts: Vec<&str> = l.trim_start_matches([' ', '*']).split_whitespace().collect();
            (parts.len() >= 3).then(|| Distro {
                name: parts[0].into(),
                running: running.iter().any(|r| r == parts[0]),
                version: parts[parts.len() - 1].into(),
                default,
            })
        })
        .collect()
}

pub(crate) fn linux_mem(meminfo: &str) -> Option<LinuxMem> {
    let m = crate::pressure::parse_meminfo(meminfo);
    if m.total == 0 {
        return None;
    }
    let cache = m.cached + m.buffers + m.reclaimable;
    Some(LinuxMem { total: m.total, used: m.total.saturating_sub(m.free).saturating_sub(cache), cache, free: m.free })
}

fn wsl_exe() -> Option<std::path::PathBuf> {
    if !cfg!(windows) || util::fake_home().is_some() {
        return None;
    }
    which("wsl")
}

/// Names of running distros. Prefer a real distro over Docker's helpers
/// when reading the VM: every distro shares one kernel, so any will do.
fn running_distros(exe: &Path) -> Vec<String> {
    run_out(exe, &["-l", "--running", "-q"], Duration::from_secs(8))
        .filter(|(ok, _, _)| *ok)
        .map(|(_, o, _)| o.lines().map(|l| l.trim().trim_matches('\0').to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn pick(running: &[String]) -> Option<String> {
    running.iter().find(|d| !d.starts_with("docker-desktop")).or(running.first()).cloned()
}

fn vm_memory() -> Option<u64> {
    let ps = which("powershell")?;
    let out = util::run_capture(
        &ps,
        &["-NoProfile", "-NonInteractive", "-Command", "(Get-Process -Name 'vmmem','VmmemWSL' -ErrorAction SilentlyContinue | Measure-Object WorkingSet64 -Sum).Sum"],
        Duration::from_secs(10),
    )?;
    out.trim().parse::<u64>().ok().filter(|n| *n > 0)
}

fn wsl_status_now() -> WslStatus {
    let path = util::home().join(".wslconfig");
    let config = wslconfig_from(&path, std::fs::read_to_string(&path).ok());
    let Some(exe) = wsl_exe() else { return WslStatus { config, ..Default::default() } };
    let Some((ok, list, _)) = run_out(&exe, &["-l", "-v"], Duration::from_secs(8)) else { return WslStatus { config, ..Default::default() } };
    let running = running_distros(&exe);
    let distros = if ok { parse_wsl_list(&list, &running) } else { vec![] };
    let linux = pick(&running).and_then(|d| {
        run_out(&exe, &["-d", &d, "-e", "cat", "/proc/meminfo"], Duration::from_secs(8)).filter(|(ok, _, _)| *ok).and_then(|(_, o, _)| linux_mem(&o))
    });
    WslStatus {
        available: ok && !distros.is_empty(),
        running: !running.is_empty(),
        distros,
        vm_memory: if running.is_empty() { None } else { vm_memory() },
        linux,
        config,
    }
}

#[tauri::command]
pub async fn wsl_status() -> R<WslStatus> {
    blocking(wsl_status_now).await
}

/// Drop the Linux page cache inside the running VM and compact memory, so
/// WSL can hand the freed pages back to Windows.
#[tauri::command]
pub async fn wsl_reclaim() -> R<WslStatus> {
    blocking(|| {
        let exe = wsl_exe().ok_or("WSL is not available")?;
        let d = pick(&running_distros(&exe)).ok_or("WSL is not running")?;
        let script = "sync; echo 3 > /proc/sys/vm/drop_caches; echo 1 > /proc/sys/vm/compact_memory 2>/dev/null; true";
        match run_out(&exe, &["-d", &d, "-u", "root", "-e", "sh", "-c", script], Duration::from_secs(60)) {
            Some((true, _, _)) => {}
            Some((false, _, err)) => return Err(format!("WSL refused: {}", err.trim())),
            None => return Err("WSL did not answer in time".into()),
        }
        // Windows takes the freed pages back over a moment.
        std::thread::sleep(Duration::from_millis(1500));
        Ok(wsl_status_now())
    })
    .await?
}

#[tauri::command]
pub async fn wsl_shutdown() -> R<WslStatus> {
    blocking(|| {
        let exe = wsl_exe().ok_or("WSL is not available")?;
        match run_out(&exe, &["--shutdown"], Duration::from_secs(60)) {
            Some((true, _, _)) => Ok(wsl_status_now()),
            Some((false, _, err)) => Err(format!("wsl --shutdown failed: {}", err.trim())),
            None => Err("wsl --shutdown did not finish in time".into()),
        }
    })
    .await?
}

// ------------------------------------------------------------- Docker

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub state: String,
    pub memory: Option<u64>,
    pub memory_limit: Option<u64>,
    pub cpu: Option<f32>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Containers {
    /// The docker CLI is installed.
    pub available: bool,
    /// The daemon answered.
    pub running: bool,
    pub error: Option<String>,
    pub items: Vec<Container>,
}

/// "1.5GiB", "512MiB", "20kB" → bytes.
pub(crate) fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (n, unit) = s.split_at(split);
    let v: f64 = n.parse().ok()?;
    let mul: f64 = match unit.trim() {
        "B" => 1.0,
        "kB" | "KB" => 1e3,
        "KiB" => 1024.0,
        "MB" => 1e6,
        "MiB" => 1024.0 * 1024.0,
        "GB" => 1e9,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TB" => 1e12,
        "TiB" => 1024f64.powi(4),
        _ => return None,
    };
    Some((v * mul) as u64)
}

pub(crate) fn parse_containers(ps: &str, stats: &str) -> Vec<Container> {
    let rows = |text: &str| -> Vec<serde_json::Value> { text.lines().filter_map(|l| serde_json::from_str(l.trim()).ok()).collect() };
    let stats = rows(stats);
    let s = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    let mut out: Vec<Container> = rows(ps)
        .iter()
        .map(|c| {
            let id = s(c, "ID");
            let st = stats.iter().find(|x| {
                let sid = s(x, "ID");
                !sid.is_empty() && (sid.starts_with(&id) || id.starts_with(&sid))
            });
            let usage = st.map(|x| s(x, "MemUsage")).unwrap_or_default();
            let mut parts = usage.split('/');
            Container {
                name: s(c, "Names"),
                image: s(c, "Image"),
                status: s(c, "Status"),
                state: s(c, "State"),
                memory: parts.next().and_then(parse_size),
                memory_limit: parts.next().and_then(parse_size),
                cpu: st.and_then(|x| s(x, "CPUPerc").trim_end_matches('%').parse().ok()),
                id,
            }
        })
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.memory.unwrap_or(0)));
    out
}

fn docker() -> Option<std::path::PathBuf> {
    if util::fake_home().is_some() {
        return None;
    }
    which("docker")
}

#[tauri::command]
pub async fn list_containers() -> R<Containers> {
    blocking(|| {
        let Some(exe) = docker() else { return Containers::default() };
        let fmt = "{{json .}}";
        match run_out(&exe, &["ps", "--format", fmt], Duration::from_secs(10)) {
            None => Containers { available: true, error: Some("Docker did not answer in time".into()), ..Default::default() },
            Some((false, _, err)) => Containers {
                available: true,
                error: Some(err.lines().find(|l| !l.trim().is_empty()).unwrap_or("Docker is not running").trim().to_string()),
                ..Default::default()
            },
            Some((true, ps, _)) => {
                let stats = if ps.trim().is_empty() {
                    String::new()
                } else {
                    run_out(&exe, &["stats", "--no-stream", "--format", fmt], Duration::from_secs(20)).map(|(_, o, _)| o).unwrap_or_default()
                };
                Containers { available: true, running: true, error: None, items: parse_containers(&ps, &stats) }
            }
        }
    })
    .await
}

/// Stop the given containers; how many stopped.
#[tauri::command]
pub async fn stop_containers(ids: Vec<String>) -> R<u32> {
    blocking(move || {
        let exe = docker().ok_or("Docker is not available")?;
        let ids: Vec<String> = ids.into_iter().filter(|i| !i.is_empty() && i.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c))).collect();
        if ids.is_empty() {
            return Ok(0);
        }
        let mut args = vec!["stop"];
        args.extend(ids.iter().map(String::as_str));
        match run_out(&exe, &args, Duration::from_secs(120)) {
            Some((_, out, err)) => {
                let n = out.lines().filter(|l| !l.trim().is_empty()).count() as u32;
                if n == 0 {
                    Err(err.trim().to_string())
                } else {
                    Ok(n)
                }
            }
            None => Err("docker stop did not finish in time".into()),
        }
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, parent: Option<u32>, start: u64, name: &str, cmd: &str) -> ProcInfo {
        ProcInfo { pid, parent, start, name: name.into(), cmd: cmd.into(), memory: 100 }
    }

    #[test]
    fn windows_orphans_have_a_missing_or_reused_parent() {
        let procs = vec![
            p(100, Some(1), 10, "Code.exe", "code"),
            p(200, Some(100), 20, "node.exe", "node tsserver.js"),            // live parent
            p(300, Some(999), 20, "node.exe", "node mcp-server.js"),          // parent gone
            p(400, Some(500), 20, "python.exe", "python -m uvicorn app:app"), // parent started later: reused
            p(500, Some(1), 50, "cmd.exe", "cmd"),
            p(600, Some(999), 20, "notepad.exe", "notepad"), // not a dev runtime
        ];
        let o = find_orphans(&procs, false, 1);
        let pids: Vec<u32> = o.iter().map(|x| x.pid).collect();
        assert_eq!(pids.len(), 2);
        assert!(pids.contains(&300) && pids.contains(&400));
        assert_eq!(o.iter().find(|x| x.pid == 300).unwrap().group, "mcp");
        assert_eq!(o.iter().find(|x| x.pid == 400).unwrap().group, "dev");
        assert!(o.iter().find(|x| x.pid == 400).unwrap().why.contains("reused"));
    }

    #[test]
    fn unix_orphans_are_adopted_by_init() {
        let procs = vec![p(1, None, 0, "launchd", ""), p(50, Some(1), 5, "node", "node vite"), p(60, Some(70), 5, "node", "node x"), p(70, Some(1), 1, "zsh", "zsh")];
        let o = find_orphans(&procs, true, 9999);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].pid, 50);
    }

    #[test]
    fn groups_by_command_line() {
        assert_eq!(group_of("node C:/x/@modelcontextprotocol/server-filesystem/dist/index.js"), "mcp");
        assert_eq!(group_of("node ~/.vscode/extensions/ms-python.vscode-pylance/dist/server.bundle.js"), "lsp");
        assert_eq!(group_of("node node_modules/typescript/lib/tsserver.js"), "lsp");
        assert_eq!(group_of("node node_modules/.bin/vite --port 5173"), "dev");
        assert_eq!(group_of("python script.py"), "other");
    }

    #[test]
    fn parses_wslconfig_from_either_section() {
        let c = wslconfig_from(Path::new("x"), Some("[wsl2]\nmemory=24GB # cap\nprocessors = 8\n\n[experimental]\nautoMemoryReclaim=gradual\nsparseVhd=true\n".into()));
        assert!(c.exists);
        assert_eq!(c.memory.as_deref(), Some("24GB"));
        assert_eq!(c.processors.as_deref(), Some("8"));
        assert_eq!(c.auto_memory_reclaim.as_deref(), Some("gradual"));
        assert_eq!(c.sparse_vhd.as_deref(), Some("true"));
        assert!(!wslconfig_from(Path::new("x"), None).exists);
    }

    #[test]
    fn parses_wsl_list_and_vm_meminfo() {
        let d = parse_wsl_list("  NAME              STATE           VERSION\n* Ubuntu-24.04      Running         2\n  docker-desktop    Stopped         2\n", &["Ubuntu-24.04".into()]);
        assert_eq!(d.len(), 2);
        assert!(d[0].default && d[0].running && d[0].version == "2");
        assert!(!d[1].running);
        let m = linux_mem("MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 300 kB\nSReclaimable: 50 kB\n").unwrap();
        assert_eq!(m.cache, 400 * 1024);
        assert_eq!(m.used, 500 * 1024);
    }

    #[test]
    fn parses_docker_json_lines() {
        let ps = r#"{"ID":"abc123def456","Image":"postgres:16","Names":"db","State":"running","Status":"Up 3 hours"}
{"ID":"0011aabbccdd","Image":"redis","Names":"cache","State":"running","Status":"Up 2 days"}"#;
        let stats = r#"{"ID":"abc123def456","Name":"db","MemUsage":"1.5GiB / 15.6GiB","CPUPerc":"0.50%"}
{"ID":"0011aabbccdd","Name":"cache","MemUsage":"20MiB / 15.6GiB","CPUPerc":"0.10%"}"#;
        let c = parse_containers(ps, stats);
        assert_eq!(c[0].name, "db");
        assert_eq!(c[0].memory, Some((1.5 * 1024.0 * 1024.0 * 1024.0) as u64));
        assert_eq!(c[0].cpu, Some(0.5));
        assert_eq!(c[1].memory, Some(20 * 1024 * 1024));
        assert_eq!(parse_size("20kB"), Some(20_000));
        assert_eq!(parse_size("junk"), None);
        assert!(parse_containers("", "").is_empty());
    }

    #[test]
    #[ignore]
    fn live_probe() {
        let o = find_orphans(&read_procs(), false, std::process::id());
        for x in o.iter().take(15) {
            println!("{} {} {} {}MB {}", x.group, x.name, x.pid, x.memory / 1048576, x.cmd.chars().take(90).collect::<String>());
        }
        println!("orphans {}", o.len());
        let w = wsl_status_now();
        println!("wsl avail={} running={} vm={:?} linux={:?} amr={:?} distros={:?}", w.available, w.running, w.vm_memory, w.linux, w.config.auto_memory_reclaim, w.distros);
    }
}
