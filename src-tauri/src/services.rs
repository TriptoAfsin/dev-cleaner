//! Background services: what is installed, whether it runs, how it starts,
//! and changing that. Dev servers (databases, Docker, web servers) that start
//! with the machine are the usual reason to come here: set them to Manual
//! and start them when you need them.
//!
//! - Windows: every service (Win32_Service). Changes run through PowerShell,
//!   elevated with one UAC prompt per change unless the app runs as admin.
//! - macOS: Homebrew services (your account, no prompt) and third-party
//!   LaunchDaemons (password prompt). Apple's own daemons are never listed.
//! - Linux: systemd system services, changed through pkexec.
//!
//! Services the OS needs (definitions/services.json "protected") are listed
//! but can't be changed: the backend refuses, whatever the UI sends. Under
//! the e2e fake home the list is read-only.

use crate::util;
use serde::Serialize;
#[cfg(unix)]
#[allow(unused_imports)]
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// The service's own name (wuauserv, postgresql-x64-16, homebrew.mxcl.redis).
    pub name: String,
    pub display_name: String,
    pub description: String,
    /// "running", "stopped", "starting", "stopping", "paused" or "unknown".
    pub state: String,
    /// "auto", "delayed", "manual", "disabled" or "unknown".
    pub start_type: String,
    pub pid: Option<u32>,
    /// Memory of its process, when it has one to itself.
    pub memory: Option<u64>,
    pub path: String,
    pub account: String,
    /// "protected", "dev", "optional" or "unknown".
    pub verdict: String,
    pub reason: String,
    pub can_change: bool,
    /// Changing it asks for admin rights (UAC, password, pkexec).
    pub needs_admin: bool,
    /// Where it comes from on macOS ("brew" or "daemon"); empty elsewhere.
    pub kind: String,
}

fn classify(name: &str, display: &str) -> (String, String) {
    let d = &crate::defs::get().services;
    for (table, verdict) in [(&d.protected, "protected"), (&d.dev, "dev"), (&d.optional, "optional")] {
        if let Some(r) = crate::defs::lookup(table, name).or_else(|| crate::defs::lookup(table, display)) {
            return (verdict.into(), r.into());
        }
    }
    ("unknown".into(), String::new())
}

/// Fill verdicts and memory, and sort: running first, then by name.
fn finish(mut v: Vec<Service>, memory: impl Fn(u32) -> Option<u64>) -> Vec<Service> {
    // A process shared by several services (svchost) has no memory of its own.
    let mut count: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for s in &v {
        if let Some(p) = s.pid.filter(|p| *p > 0) {
            *count.entry(p).or_default() += 1;
        }
    }
    for s in &mut v {
        let (verdict, reason) = classify(&s.name, &s.display_name);
        s.can_change = verdict != "protected";
        s.verdict = verdict;
        if s.reason.is_empty() {
            s.reason = reason;
        }
        s.memory = s.pid.filter(|p| *p > 0 && count.get(p) == Some(&1)).and_then(&memory);
    }
    v.sort_by(|a, b| (a.state != "running").cmp(&(b.state != "running")).then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase())));
    v
}

pub fn list(memory: impl Fn(u32) -> Option<u64>) -> Vec<Service> {
    finish(os::list(), memory)
}

/// "start", "stop", "restart", "auto", "manual" or "disabled" (which also
/// stops it). Returns the service as it is afterwards.
pub fn act(name: &str, action: &str, memory: impl Fn(u32) -> Option<u64>) -> Result<Service, String> {
    if !matches!(action, "start" | "stop" | "restart" | "auto" | "manual" | "disabled") {
        return Err("Unknown action".into());
    }
    let all = list(&memory);
    let s = all.into_iter().find(|s| s.name == name).ok_or("That service is gone; refresh the list")?;
    if !s.can_change {
        return Err(format!("{} is protected: the system needs it", s.display_name));
    }
    if util::fake_home().is_some() {
        return Err("Service changes are off in tests".into());
    }
    os::act(&s, action)?;
    list(&memory).into_iter().find(|x| x.name == name).ok_or_else(|| "Changed, but the service can't be read back".into())
}

fn svc(name: String, display: String) -> Service {
    Service {
        display_name: if display.is_empty() { name.clone() } else { display },
        name,
        description: String::new(),
        state: "unknown".into(),
        start_type: "unknown".into(),
        pid: None,
        memory: None,
        path: String::new(),
        account: String::new(),
        verdict: String::new(),
        reason: String::new(),
        can_change: false,
        needs_admin: true,
        kind: String::new(),
    }
}

// -------------------------------------------------------------------- Windows

#[cfg(windows)]
mod os {
    use super::*;
    use crate::junk::ps_quote;

    const LIST: &str = "$ErrorActionPreference = 'SilentlyContinue'; ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Service | Select-Object Name, DisplayName, Description, State, StartMode, DelayedAutoStart, ProcessId, PathName, StartName)";

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Raw {
        name: String,
        display_name: Option<String>,
        description: Option<String>,
        state: Option<String>,
        start_mode: Option<String>,
        delayed_auto_start: Option<bool>,
        process_id: Option<u32>,
        path_name: Option<String>,
        start_name: Option<String>,
    }

    pub fn list() -> Vec<Service> {
        let Some(ps) = util::which("powershell") else { return vec![] };
        let Some(text) = util::run_capture(&ps, &["-NoProfile", "-NonInteractive", "-Command", LIST], Duration::from_secs(30)) else { return vec![] };
        let raw: Vec<Raw> = serde_json::from_str(text.trim()).unwrap_or_default();
        let admin = crate::sysinfo_dev::is_admin();
        raw.into_iter()
            .map(|r| {
                let mut s = svc(r.name, r.display_name.unwrap_or_default());
                s.description = r.description.unwrap_or_default();
                s.state = match r.state.as_deref() {
                    Some("Running") => "running",
                    Some("Stopped") => "stopped",
                    Some("Start Pending") => "starting",
                    Some("Stop Pending") => "stopping",
                    Some("Paused") => "paused",
                    _ => "unknown",
                }
                .into();
                s.start_type = match (r.start_mode.as_deref(), r.delayed_auto_start) {
                    (Some("Auto"), Some(true)) => "delayed",
                    (Some("Auto"), _) => "auto",
                    (Some("Manual"), _) => "manual",
                    (Some("Disabled"), _) => "disabled",
                    _ => "unknown",
                }
                .into();
                s.pid = r.process_id.filter(|p| *p > 0);
                s.path = r.path_name.unwrap_or_default();
                s.account = r.start_name.unwrap_or_default();
                s.needs_admin = !admin;
                s
            })
            .collect()
    }

    pub fn act(s: &Service, action: &str) -> Result<(), String> {
        let n = ps_quote(&s.name);
        let body = match action {
            "start" => format!("Start-Service -Name {n}"),
            "stop" => format!("Stop-Service -Name {n} -Force"),
            "restart" => format!("Restart-Service -Name {n} -Force"),
            "auto" => format!("Set-Service -Name {n} -StartupType Automatic"),
            "manual" => format!("Set-Service -Name {n} -StartupType Manual"),
            // Disabling also stops it now, so the memory comes back at once.
            "disabled" => format!("Set-Service -Name {n} -StartupType Disabled; Stop-Service -Name {n} -Force -ErrorAction SilentlyContinue"),
            _ => return Err("Unknown action".into()),
        };
        let script = format!("$ErrorActionPreference = 'Stop'\ntry {{ {body} }} catch {{ exit 1 }}\nexit 0");
        if crate::junk::run_elevated(&script) {
            Ok(())
        } else {
            Err("Cancelled, or Windows refused the change. A service that other running services depend on may refuse to stop.".into())
        }
    }
}

// ---------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod os {
    use super::*;
    use crate::junk::sh_quote;

    fn brew() -> Option<PathBuf> {
        util::which("brew")
    }

    fn plist(path: &Path) -> Option<serde_json::Value> {
        let p = path.to_string_lossy();
        let text = util::run_capture(Path::new("/usr/bin/plutil"), &["-convert", "json", "-o", "-", &p], Duration::from_secs(5))?;
        serde_json::from_str(&text).ok()
    }

    pub fn list() -> Vec<Service> {
        let mut out = vec![];
        // Homebrew services: postgres, redis, mysql... run in your account.
        if let Some(b) = brew() {
            if let Some(text) = util::run_capture(&b, &["services", "list", "--json"], Duration::from_secs(20)) {
                let rows: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
                for r in rows {
                    let name = r.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let status = r.get("status").and_then(|x| x.as_str()).unwrap_or("none");
                    let mut s = svc(format!("homebrew.mxcl.{name}"), name.clone());
                    s.kind = "brew".into();
                    s.state = if status == "started" { "running" } else { "stopped" }.into();
                    // brew starts a service at login once it has been started.
                    s.start_type = if status == "started" || status == "scheduled" { "auto" } else { "manual" }.into();
                    s.path = r.get("file").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    s.account = r.get("user").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    s.description = format!("Homebrew service for {name}");
                    s.needs_admin = false;
                    out.push(s);
                }
            }
        }
        // Third-party system daemons. Apple's are never listed.
        let disabled = util::run_capture(Path::new("/bin/launchctl"), &["print-disabled", "system"], Duration::from_secs(10)).unwrap_or_default();
        if let Ok(rd) = std::fs::read_dir("/Library/LaunchDaemons") {
            for e in rd.flatten() {
                let path = e.path();
                let Some(p) = plist(&path) else { continue };
                let label = p.get("Label").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if label.is_empty() || label.starts_with("com.apple.") {
                    continue;
                }
                let off = disabled.lines().any(|l| l.contains(&format!("\"{label}\"")) && (l.contains("disabled") || l.contains("true")));
                let mut s = svc(label.clone(), label.clone());
                s.kind = "daemon".into();
                s.start_type = if off { "disabled" } else if p.get("RunAtLoad").and_then(|x| x.as_bool()).unwrap_or(false) { "auto" } else { "manual" }.into();
                s.path = path.to_string_lossy().into();
                s.description = p
                    .get("ProgramArguments")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "))
                    .or_else(|| p.get("Program").and_then(|x| x.as_str()).map(String::from))
                    .unwrap_or_default();
                out.push(s);
            }
        }
        out
    }

    pub fn act(s: &Service, action: &str) -> Result<(), String> {
        if s.kind == "brew" {
            let b = brew().ok_or("Homebrew not found")?;
            let name = s.display_name.as_str();
            let args: &[&str] = match action {
                "start" | "auto" => &["services", "start", name],
                "stop" | "manual" | "disabled" => &["services", "stop", name],
                "restart" => &["services", "restart", name],
                _ => return Err("Unknown action".into()),
            };
            return if util::run_ok(&b, args, Duration::from_secs(60)) { Ok(()) } else { Err(format!("brew services {} failed", args[1])) };
        }
        let label = sh_quote(&format!("system/{}", s.name));
        let file = sh_quote(&s.path);
        let script = match action {
            "start" => format!("launchctl kickstart {label}"),
            "stop" => format!("launchctl kill TERM {label}"),
            "restart" => format!("launchctl kickstart -k {label}"),
            "auto" | "manual" => format!("launchctl enable {label}; launchctl bootstrap system {file} 2>/dev/null; true"),
            "disabled" => format!("launchctl bootout {label} 2>/dev/null; launchctl disable {label}"),
            _ => return Err("Unknown action".into()),
        };
        if crate::junk::run_elevated(&script) { Ok(()) } else { Err("Cancelled, or macOS refused the change".into()) }
    }
}

// ---------------------------------------------------------------------- Linux

#[cfg(all(unix, not(target_os = "macos")))]
mod os {
    use super::*;
    use std::collections::HashMap;

    fn systemctl() -> Option<PathBuf> {
        util::which("systemctl")
    }

    pub fn list() -> Vec<Service> {
        let Some(sc) = systemctl() else { return vec![] };
        // Unit files: how each starts.
        let files = util::run_capture(&sc, &["list-unit-files", "--type=service", "--no-legend", "--no-pager", "--plain"], Duration::from_secs(20)).unwrap_or_default();
        let mut start: HashMap<String, String> = HashMap::new();
        for l in files.lines() {
            let mut it = l.split_whitespace();
            if let (Some(unit), Some(state)) = (it.next(), it.next()) {
                start.insert(unit.to_string(), state.to_string());
            }
        }
        // Loaded units: whether each runs, and its description.
        let units = util::run_capture(&sc, &["list-units", "--type=service", "--all", "--no-legend", "--no-pager", "--plain"], Duration::from_secs(20)).unwrap_or_default();
        let mut out = vec![];
        let mut seen = std::collections::HashSet::new();
        for l in units.lines() {
            let mut it = l.split_whitespace();
            let (Some(unit), Some(_load), Some(_active), Some(sub)) = (it.next(), it.next(), it.next(), it.next()) else { continue };
            let desc: Vec<&str> = it.collect();
            let name = unit.trim_end_matches(".service").to_string();
            seen.insert(unit.to_string());
            let mut s = svc(name, desc.join(" "));
            s.state = match sub {
                "running" => "running",
                "exited" | "dead" | "failed" => "stopped",
                "activating" | "start" => "starting",
                "deactivating" => "stopping",
                _ => "unknown",
            }
            .into();
            s.start_type = start_type(start.get(unit).map(String::as_str));
            out.push(s);
        }
        for (unit, state) in &start {
            if seen.contains(unit) || unit.ends_with("@.service") {
                continue;
            }
            let mut s = svc(unit.trim_end_matches(".service").to_string(), String::new());
            s.state = "stopped".into();
            s.start_type = start_type(Some(state));
            out.push(s);
        }
        out
    }

    fn start_type(s: Option<&str>) -> String {
        match s {
            Some("enabled") | Some("enabled-runtime") => "auto",
            Some("disabled") | Some("static") | Some("indirect") => "manual",
            Some("masked") | Some("masked-runtime") => "disabled",
            _ => "unknown",
        }
        .into()
    }

    pub fn act(s: &Service, action: &str) -> Result<(), String> {
        let unit = crate::junk::sh_quote(&format!("{}.service", s.name));
        let script = match action {
            "start" => format!("systemctl start {unit}"),
            "stop" => format!("systemctl stop {unit}"),
            "restart" => format!("systemctl restart {unit}"),
            "auto" => format!("systemctl unmask {unit} && systemctl enable {unit}"),
            "manual" => format!("systemctl unmask {unit} && systemctl disable {unit}"),
            "disabled" => format!("systemctl disable --now {unit} && systemctl mask {unit}"),
            _ => return Err("Unknown action".into()),
        };
        if crate::junk::run_elevated(&script) { Ok(()) } else { Err("Cancelled, or systemd refused the change".into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_services_cant_change_and_dev_servers_are_flagged() {
        let mk = |n: &str| {
            let mut s = svc(n.into(), n.into());
            s.pid = Some(7);
            s
        };
        let v = finish(vec![mk("RpcSs"), mk("postgresql-x64-16"), mk("SomethingElse")], |_| Some(1));
        let get = |n: &str| v.iter().find(|s| s.name == n).unwrap();
        assert!(!get("RpcSs").can_change);
        assert_eq!(get("RpcSs").verdict, "protected");
        assert_eq!(get("postgresql-x64-16").verdict, "dev");
        assert!(get("SomethingElse").can_change);
        // Three services share pid 7: none gets that process's memory.
        assert!(v.iter().all(|s| s.memory.is_none()));
    }

    #[test]
    fn unknown_actions_are_refused() {
        assert!(act("RpcSs", "delete", |_| None).is_err());
    }
}

