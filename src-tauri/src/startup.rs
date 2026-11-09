//! Programs that start when you sign in, and turning them off or on.
//!
//! Nothing is deleted. Each OS's own switch is used, so the change shows up
//! in its own settings too:
//! - Windows: the `Run` registry keys and the Startup folders, switched with
//!   `StartupApproved`, exactly as Task Manager's Startup tab does.
//!   All-users entries need admin rights.
//! - macOS: LaunchAgents, switched with `launchctl disable/enable` in your
//!   session.
//! - Linux: XDG autostart entries, switched with `Hidden=` in your own copy.
//!
//! Under the e2e fake home only `<fake>/roaming/.../Startup` is read and the
//! on/off state lives in a file there, so tests never touch the registry.

use crate::util;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    /// Stable id; the backend finds the entry again from it, never trusting
    /// anything else the frontend sends.
    pub id: String,
    /// The entry's own name (registry value, shortcut, label).
    pub name: String,
    /// What to show: the program's description when it has one.
    pub display_name: String,
    /// Who made it, from the program's version info.
    pub publisher: Option<String>,
    pub command: String,
    /// Where it is registered, in plain words ("Registry · your account").
    pub location: String,
    /// "user" or "machine" (every account; needs admin to change).
    pub scope: String,
    pub enabled: bool,
    pub needs_admin: bool,
    /// "keep", "optional" or "unknown".
    pub verdict: String,
    pub reason: String,
    /// The file behind it, when there is one (shortcut, plist, .desktop).
    pub path: Option<String>,
}

fn verdict(name: &str, command: &str) -> (String, String) {
    let d = &crate::defs::get().startup;
    let exe = Path::new(command.trim_matches('"').split(".exe").next().unwrap_or(""))
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    for key in [name, exe.as_str()] {
        if key.is_empty() {
            continue;
        }
        if let Some(r) = crate::defs::lookup(&d.keep, key) {
            return ("keep".into(), r.into());
        }
    }
    for key in [name, exe.as_str()] {
        if key.is_empty() {
            continue;
        }
        if let Some(r) = crate::defs::lookup(&d.optional, key) {
            return ("optional".into(), r.into());
        }
    }
    ("unknown".into(), "Not recognised; check what it is before turning it off".into())
}

fn item(id: String, name: String, command: String, location: &str, scope: &str, enabled: bool, path: Option<PathBuf>) -> StartupItem {
    let (verdict, reason) = verdict(&name, &command);
    StartupItem {
        id,
        display_name: name.clone(),
        publisher: None,
        name,
        command,
        location: location.into(),
        needs_admin: scope == "machine" && cfg!(windows),
        scope: scope.into(),
        enabled,
        verdict,
        reason,
        path: path.map(|p| p.to_string_lossy().into_owned()),
    }
}

pub fn list() -> Vec<StartupItem> {
    let mut v = if util::fake_home().is_some() { fake::list() } else { os::list() };
    v.sort_by(|a, b| b.enabled.cmp(&a.enabled).then_with(|| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase())));
    v
}

/// Turn an entry on or off. Returns the entry as it is afterwards.
pub fn set_enabled(id: &str, enabled: bool) -> Result<StartupItem, String> {
    let current = list().into_iter().find(|i| i.id == id).ok_or("That startup entry is gone; refresh the list")?;
    if util::fake_home().is_some() {
        fake::set(&current, enabled)?;
    } else {
        os::set(&current, enabled)?;
    }
    list().into_iter().find(|i| i.id == id).ok_or_else(|| "Changed, but the entry can't be read back".into())
}

// ------------------------------------------------------------------ fake home

mod fake {
    use super::*;

    fn folder() -> PathBuf {
        util::roaming_dir().join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Startup")
    }

    fn state_file() -> PathBuf {
        util::fake_home().unwrap_or_default().join("startup-approved.json")
    }

    fn state() -> HashMap<String, bool> {
        std::fs::read_to_string(state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn list() -> Vec<StartupItem> {
        let st = state();
        let Ok(rd) = std::fs::read_dir(folder()) else { return vec![] };
        rd.flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| {
                let file = e.file_name().to_string_lossy().to_string();
                let name = Path::new(&file).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(file.clone());
                let enabled = st.get(&file).copied().unwrap_or(true);
                item(format!("folder:user:{file}"), name, e.path().to_string_lossy().into(), "Startup folder · your account", "user", enabled, Some(e.path()))
            })
            .collect()
    }

    pub fn set(it: &StartupItem, enabled: bool) -> Result<(), String> {
        let file = it.id.rsplit(':').next().unwrap_or("").to_string();
        let mut st = state();
        st.insert(file, enabled);
        std::fs::write(state_file(), serde_json::to_vec_pretty(&st).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }
}

// -------------------------------------------------------------------- Windows

#[cfg(windows)]
mod os {
    use super::*;
    use crate::junk::ps_quote;

    /// One PowerShell call reads everything: Run keys (user, all users,
    /// 32-bit all users), both Startup folders (shortcut targets resolved),
    /// and each entry's StartupApproved state.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$out = New-Object System.Collections.ArrayList
$approved = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved'
function On($hive, $sub, $name) {
  $k = Get-Item -LiteralPath "Registry::$hive\$approved\$sub"
  if (-not $k) { return $true }
  $v = $k.GetValue($name)
  if ($v -eq $null -or $v.Length -eq 0) { return $true }
  return (($v[0] -band 1) -eq 0)
}
$runs = @(
  @('HKEY_CURRENT_USER', 'Software\Microsoft\Windows\CurrentVersion\Run', 'Run', 'user'),
  @('HKEY_LOCAL_MACHINE', 'SOFTWARE\Microsoft\Windows\CurrentVersion\Run', 'Run', 'machine'),
  @('HKEY_LOCAL_MACHINE', 'SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Run', 'Run32', 'machine')
)
foreach ($r in $runs) {
  $k = Get-Item -LiteralPath "Registry::$($r[0])\$($r[1])"
  if (-not $k) { continue }
  foreach ($n in $k.GetValueNames()) {
    if ($n -eq '') { continue }
    [void]$out.Add([pscustomobject]@{ kind = 'run'; hive = $r[0]; sub = $r[2]; scope = $r[3]; name = $n; file = $n; command = [string]$k.GetValue($n); path = $null; enabled = (On $r[0] $r[2] $n) })
  }
}
function Info($cmd) {
  $c = $cmd.Trim()
  if ($c.StartsWith('"')) { $exe = $c.Substring(1, [Math]::Max(0, $c.IndexOf('"', 1) - 1)) }
  else { $i = $c.ToLower().IndexOf('.exe'); $exe = if ($i -gt 0) { $c.Substring(0, $i + 4) } else { $c } }
  $exe = [Environment]::ExpandEnvironmentVariables($exe)
  if ($exe -and (Test-Path -LiteralPath $exe -PathType Leaf)) { $v = (Get-Item -LiteralPath $exe).VersionInfo; return @($v.FileDescription, $v.CompanyName) }
  return @($null, $null)
}
foreach ($o in $out) { $x = Info $o.command; $o | Add-Member desc $x[0]; $o | Add-Member company $x[1] }
$shell = New-Object -ComObject WScript.Shell
foreach ($f in @(@([Environment]::GetFolderPath('Startup'), 'HKEY_CURRENT_USER', 'user'), @([Environment]::GetFolderPath('CommonStartup'), 'HKEY_LOCAL_MACHINE', 'machine'))) {
  if (-not $f[0] -or -not (Test-Path -LiteralPath $f[0])) { continue }
  foreach ($e in Get-ChildItem -LiteralPath $f[0] -File -Force) {
    if ($e.Name -eq 'desktop.ini') { continue }
    $cmd = $e.FullName
    if ($e.Extension -eq '.lnk') { $l = $shell.CreateShortcut($e.FullName); if ($l.TargetPath) { $cmd = ('"' + $l.TargetPath + '" ' + $l.Arguments).Trim() } }
    $x = Info $cmd
    [void]$out.Add([pscustomobject]@{ kind = 'folder'; hive = $f[1]; sub = 'StartupFolder'; scope = $f[2]; name = $e.BaseName; file = $e.Name; command = $cmd; path = $e.FullName; enabled = (On $f[1] 'StartupFolder' $e.Name); desc = $x[0]; company = $x[1] })
  }
}
ConvertTo-Json -InputObject @($out) -Compress -Depth 3
"#;

    #[derive(serde::Deserialize)]
    struct Raw {
        kind: String,
        hive: String,
        sub: String,
        scope: String,
        name: String,
        file: String,
        command: String,
        path: Option<String>,
        enabled: bool,
        desc: Option<String>,
        company: Option<String>,
    }

    fn powershell() -> Option<PathBuf> {
        util::which("powershell")
    }

    pub fn list() -> Vec<StartupItem> {
        let Some(ps) = powershell() else { return vec![] };
        let Some(text) = util::run_capture(&ps, &["-NoProfile", "-NonInteractive", "-Command", SCRIPT], Duration::from_secs(20)) else { return vec![] };
        let raw: Vec<Raw> = serde_json::from_str(text.trim()).unwrap_or_default();
        raw.into_iter()
            .map(|r| {
                let place = if r.kind == "run" { "Registry" } else { "Startup folder" };
                let whose = if r.scope == "user" { "your account" } else { "all users" };
                let id = format!("{}:{}:{}:{}", r.kind, r.hive, r.sub, r.file);
                let mut it = item(id, r.name, r.command, &format!("{place} · {whose}"), &r.scope, r.enabled, r.path.map(PathBuf::from));
                let clean = |s: Option<String>| s.map(|x| x.trim().to_string()).filter(|x| !x.is_empty());
                if let Some(d) = clean(r.desc) {
                    // Verdicts can also match the description ("Logitech G HUB").
                    if it.verdict == "unknown" {
                        let (v, why) = verdict(&d, "");
                        it.verdict = v;
                        it.reason = why;
                    }
                    it.display_name = d;
                }
                it.publisher = clean(r.company);
                it
            })
            .collect()
    }

    pub fn set(it: &StartupItem, enabled: bool) -> Result<(), String> {
        // id = kind:hive:sub:file; the file may itself contain ':'.
        let mut parts = it.id.splitn(4, ':');
        let (_, hive, sub, file) = (parts.next(), parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        if !matches!(sub, "Run" | "Run32" | "StartupFolder") || !matches!(hive, "HKEY_CURRENT_USER" | "HKEY_LOCAL_MACHINE") {
            return Err("Unknown startup entry".into());
        }
        let root = if hive == "HKEY_CURRENT_USER" { "HKCU:" } else { "HKLM:" };
        // Task Manager's format: 12 bytes, first byte 2 = on, 3 = off.
        let first = if enabled { 2 } else { 3 };
        let key = format!(r"{root}\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\{sub}");
        let script = format!(
            "$ErrorActionPreference = 'Stop'\nif (-not (Test-Path -LiteralPath {k})) {{ New-Item -Path {k} -Force | Out-Null }}\nSet-ItemProperty -LiteralPath {k} -Name {n} -Type Binary -Value ([byte[]]({first},0,0,0,0,0,0,0,0,0,0,0))\nexit 0",
            k = ps_quote(&key),
            n = ps_quote(file),
        );
        let ok = if it.scope == "user" {
            let ps = powershell().ok_or("PowerShell not found")?;
            util::run_ok(&ps, &["-NoProfile", "-NonInteractive", "-Command", &script], Duration::from_secs(20))
        } else {
            crate::junk::run_elevated(&script)
        };
        if ok {
            Ok(())
        } else if it.scope == "user" {
            Err("Windows didn't accept the change".into())
        } else {
            Err("Cancelled, or Windows refused the change. All-users entries need administrator rights.".into())
        }
    }
}

// ---------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
mod os {
    use super::*;

    fn uid() -> String {
        util::run_capture(Path::new("/usr/bin/id"), &["-u"], Duration::from_secs(5)).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    /// Labels switched off in your session, from launchctl's override list.
    fn overrides(uid: &str) -> HashMap<String, bool> {
        let text = util::run_capture(Path::new("/bin/launchctl"), &["print-disabled", &format!("gui/{uid}")], Duration::from_secs(10)).unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let (k, v) = l.split_once("=>")?;
                let label = k.trim().trim_matches('"').to_string();
                Some((label, v.trim().starts_with("disabled") || v.trim().starts_with("true")))
            })
            .collect()
    }

    fn plist(path: &Path) -> Option<serde_json::Value> {
        let p = path.to_string_lossy();
        let text = util::run_capture(Path::new("/usr/bin/plutil"), &["-convert", "json", "-o", "-", &p], Duration::from_secs(5))?;
        serde_json::from_str(&text).ok()
    }

    pub fn list() -> Vec<StartupItem> {
        let uid = uid();
        let off = overrides(&uid);
        let mut out = vec![];
        for (dir, scope) in [(util::home().join("Library/LaunchAgents"), "user"), (PathBuf::from("/Library/LaunchAgents"), "machine")] {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().is_none_or(|x| x != "plist") {
                    continue;
                }
                let Some(p) = plist(&path) else { continue };
                let label = p.get("Label").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if label.is_empty() {
                    continue;
                }
                let command = p
                    .get("ProgramArguments")
                    .and_then(|a| a.as_array())
                    .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "))
                    .or_else(|| p.get("Program").and_then(|x| x.as_str()).map(String::from))
                    .unwrap_or_default();
                let disabled_in_file = p.get("Disabled").and_then(|x| x.as_bool()).unwrap_or(false);
                let enabled = match off.get(&label) {
                    Some(d) => !d,
                    None => !disabled_in_file,
                };
                let whose = if scope == "user" { "your account" } else { "all users" };
                out.push(item(format!("agent:{label}"), label, command, &format!("LaunchAgent · {whose}"), scope, enabled, Some(path)));
            }
        }
        out
    }

    pub fn set(it: &StartupItem, enabled: bool) -> Result<(), String> {
        let label = it.id.strip_prefix("agent:").ok_or("Unknown startup entry")?;
        let target = format!("gui/{}/{label}", uid());
        let verb = if enabled { "enable" } else { "disable" };
        if util::run_ok(Path::new("/bin/launchctl"), &[verb, &target], Duration::from_secs(10)) {
            Ok(())
        } else {
            Err(format!("launchctl {verb} failed"))
        }
    }
}

// ---------------------------------------------------------------------- Linux

#[cfg(all(unix, not(target_os = "macos")))]
mod os {
    use super::*;

    fn user_dir() -> PathBuf {
        util::home().join(".config").join("autostart")
    }

    fn read(path: &Path) -> HashMap<String, String> {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let mut in_entry = false;
        let mut m = HashMap::new();
        for line in text.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                in_entry = l == "[Desktop Entry]";
                continue;
            }
            if in_entry {
                if let Some((k, v)) = l.split_once('=') {
                    m.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
                }
            }
        }
        m
    }

    fn on(m: &HashMap<String, String>) -> bool {
        let f = |k: &str| m.get(k).is_some_and(|v| v.eq_ignore_ascii_case("true"));
        !f("Hidden") && m.get("X-GNOME-Autostart-enabled").is_none_or(|v| !v.eq_ignore_ascii_case("false"))
    }

    pub fn list() -> Vec<StartupItem> {
        let mut by_file: HashMap<String, (PathBuf, &str)> = HashMap::new();
        for (dir, scope) in [(PathBuf::from("/etc/xdg/autostart"), "machine"), (user_dir(), "user")] {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|x| x == "desktop") {
                    // Your copy overrides the system one with the same name.
                    by_file.insert(e.file_name().to_string_lossy().to_string(), (p, scope));
                }
            }
        }
        by_file
            .into_iter()
            .map(|(file, (path, scope))| {
                let m = read(&path);
                let name = m.get("Name").cloned().unwrap_or_else(|| file.trim_end_matches(".desktop").to_string());
                let command = m.get("Exec").cloned().unwrap_or_default();
                let place = if scope == "user" { "Autostart · your account" } else { "Autostart · system" };
                // Switching a system entry writes your own copy: no admin needed.
                item(format!("desktop:{file}"), name, command, place, "user", on(&m), Some(path))
            })
            .collect()
    }

    pub fn set(it: &StartupItem, enabled: bool) -> Result<(), String> {
        let file = it.id.strip_prefix("desktop:").ok_or("Unknown startup entry")?;
        if file.contains('/') || file.contains("..") {
            return Err("Unknown startup entry".into());
        }
        let mine = user_dir().join(file);
        let source = if mine.is_file() { mine.clone() } else { PathBuf::from("/etc/xdg/autostart").join(file) };
        let text = std::fs::read_to_string(&source).map_err(|e| e.to_string())?;
        let value = if enabled { "false" } else { "true" };
        let mut lines: Vec<String> = text.lines().filter(|l| !l.trim_start().starts_with("Hidden=") && !l.trim_start().starts_with("X-GNOME-Autostart-enabled=")).map(String::from).collect();
        let at = lines.iter().position(|l| l.trim() == "[Desktop Entry]").map(|i| i + 1).unwrap_or(lines.len());
        lines.insert(at, format!("Hidden={value}"));
        lines.insert(at + 1, format!("X-GNOME-Autostart-enabled={}", if enabled { "true" } else { "false" }));
        std::fs::create_dir_all(user_dir()).map_err(|e| e.to_string())?;
        std::fs::write(&mine, lines.join("\n") + "\n").map_err(|e| e.to_string())
    }
}
