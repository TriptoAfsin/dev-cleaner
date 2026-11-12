//! Responsiveness: a short list of real, reversible settings that keep a dev
//! machine quick under load. Each tweak reads its current state from the
//! machine, applies one exact change, and records what it replaced in a
//! journal so it can be undone from History.
//!
//! Deliberately absent (see the "we skip" list in the UI): Defender
//! exclusions, turning off memory compression, the pagefile or SysMain, and
//! registry "speed" tweaks.
//!
//! Under the e2e fake home, file tweaks write inside the fake home only and
//! system tweaks (power plan, animations, transparency) never run.

use crate::util::{self, run_capture, which};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;

type R<T> = Result<T, String>;
const T: Duration = Duration::from_secs(10);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn sandbox() -> bool {
    util::fake_home().is_some()
}

// ---------------------------------------------------------------- types

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TuneItem {
    pub id: String,
    pub title: String,
    /// Why it applies here, from this machine.
    pub evidence: String,
    /// 1 low, 2 medium, 3 high.
    pub impact: u8,
    pub admin: bool,
    /// "Reload VS Code windows", "after wsl --shutdown"…
    pub note: String,
    /// "applied", "available" or "na".
    pub state: String,
    pub na_reason: Option<String>,
    pub detail: String,
    /// The exact change.
    pub code: String,
    /// A settings page to open instead, when it can't be applied here.
    pub link: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TuneOverview {
    pub items: Vec<TuneItem>,
    /// Settings that are already right, shown as chips.
    pub good: Vec<String>,
    pub startup_enabled: Option<u32>,
    pub dry_run: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Undo {
    Wsl { path: String, previous: Option<String>, written: String, prev_reclaim: Option<String>, prev_sparse: Option<String> },
    Editors { files: Vec<FileUndo> },
    Power { previous: String, previous_name: String },
    Animation { client: bool, minimize: bool },
    Transparency { previous: Option<u32> },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FileUndo {
    pub path: String,
    pub previous: Option<String>,
    pub written: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub id: String,
    pub tweak: String,
    pub title: String,
    pub summary: String,
    pub applied_at: u64,
    pub undone_at: Option<u64>,
    pub undo_note: Option<String>,
    pub undo: Undo,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TuneOutcome {
    pub id: String,
    pub ok: bool,
    pub message: String,
    pub entry_id: Option<String>,
    pub dry_run: bool,
}

// ---------------------------------------------------------------- journal

fn journal_path(app: &AppHandle) -> Option<PathBuf> {
    Some(crate::winstate::data_dir(app)?.join("tune-journal.json"))
}

fn load_journal(app: &AppHandle) -> Vec<JournalEntry> {
    journal_path(app).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_journal(app: &AppHandle, j: &[JournalEntry]) -> R<()> {
    let p = journal_path(app).ok_or("no data folder")?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(j).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- INI (.wslconfig)

fn ini_section(line: &str) -> Option<String> {
    let l = line.trim();
    (l.starts_with('[') && l.ends_with(']')).then(|| l[1..l.len() - 1].trim().to_ascii_lowercase())
}

/// The value of `key` in `[section]`, case-insensitive.
pub(crate) fn ini_get(text: &str, section: &str, key: &str) -> Option<String> {
    let mut cur = String::new();
    for line in text.lines() {
        if let Some(s) = ini_section(line) {
            cur = s;
            continue;
        }
        if cur != section.to_ascii_lowercase() {
            continue;
        }
        let l = line.trim();
        if l.starts_with('#') || l.starts_with(';') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim().eq_ignore_ascii_case(key) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// Set (Some) or remove (None) keys in `[section]`, keeping every other line.
pub(crate) fn ini_set(text: &str, section: &str, set: &[(&str, Option<&str>)]) -> String {
    let sec = section.to_ascii_lowercase();
    let mut out: Vec<String> = vec![];
    let mut cur = String::new();
    let mut done: Vec<bool> = vec![false; set.len()];
    let mut section_end: Option<usize> = None;
    let mut found_section = false;
    for line in text.lines() {
        if let Some(s) = ini_section(line) {
            if cur == sec {
                section_end = Some(out.len());
            }
            cur = s;
            if cur == sec {
                found_section = true;
            }
            out.push(line.to_string());
            continue;
        }
        if cur == sec {
            let l = line.trim();
            if let Some((k, _)) = l.split_once('=').filter(|_| !l.starts_with('#') && !l.starts_with(';')) {
                if let Some(i) = set.iter().position(|(key, _)| k.trim().eq_ignore_ascii_case(key)) {
                    if done[i] {
                        continue;
                    }
                    done[i] = true;
                    if let Some(v) = set[i].1 {
                        out.push(format!("{}={}", set[i].0, v));
                    }
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    if cur == sec && section_end.is_none() {
        // Section runs to the end; trim trailing blank lines into it.
        let mut at = out.len();
        while at > 0 && out[at - 1].trim().is_empty() {
            at -= 1;
        }
        section_end = Some(at);
    }
    let missing: Vec<String> = set.iter().enumerate().filter(|(i, (_, v))| !done[*i] && v.is_some()).map(|(_, (k, v))| format!("{}={}", k, v.unwrap_or(""))).collect();
    if !missing.is_empty() {
        if found_section {
            let at = section_end.unwrap_or(out.len());
            for (n, m) in missing.into_iter().enumerate() {
                out.insert(at + n, m);
            }
        } else {
            while out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(format!("[{section}]"));
            out.extend(missing);
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

fn wslconfig_path() -> PathBuf {
    util::home().join(".wslconfig")
}

fn wsl_installed() -> bool {
    if sandbox() {
        return true;
    }
    if !cfg!(windows) {
        return false;
    }
    which("reg")
        .and_then(|r| run_capture(&r, &["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss"], T))
        .is_some_and(|o| o.contains('{'))
}

fn wsl_reclaim_on(text: &str) -> bool {
    let v = ini_get(text, "experimental", "autoMemoryReclaim").or_else(|| ini_get(text, "wsl2", "autoMemoryReclaim"));
    v.is_some_and(|v| v.eq_ignore_ascii_case("dropcache") || v.eq_ignore_ascii_case("gradual"))
}

const WSL_CODE: &str = "# %UserProfile%\\.wslconfig\n[experimental]\nautoMemoryReclaim=dropCache\nsparseVhd=true";

fn wsl_item() -> TuneItem {
    let installed = wsl_installed();
    let text = std::fs::read_to_string(wslconfig_path()).unwrap_or_default();
    let mem = ini_get(&text, "wsl2", "memory");
    let applied = wsl_reclaim_on(&text);
    let evidence = if !installed {
        "WSL isn't installed.".to_string()
    } else if applied {
        "WSL already hands unused Linux cache back to Windows.".into()
    } else {
        format!(
            "WSL keeps Linux's file cache in Windows RAM until it shuts down{}.",
            mem.map(|m| format!(" (memory cap: {m})")).unwrap_or_else(|| " (no memory cap set: up to half your RAM)".into())
        )
    };
    TuneItem {
        id: "wsl".into(),
        title: "Let WSL give memory back".into(),
        evidence,
        impact: 3,
        admin: false,
        note: "Takes effect after wsl --shutdown".into(),
        state: if !installed { "na" } else if applied { "applied" } else { "available" }.into(),
        na_reason: (!installed).then(|| "WSL isn't installed.".into()),
        detail: "Docker Desktop and your distros share one Linux VM. With autoMemoryReclaim, Linux drops its file cache once the VM is idle and Windows gets that RAM back; sparseVhd lets the virtual disk shrink as files are deleted. Your other .wslconfig settings are kept.".into(),
        code: WSL_CODE.into(),
        link: None,
    }
}

fn wsl_apply() -> R<(Undo, String)> {
    let path = wslconfig_path();
    let previous = std::fs::read_to_string(&path).ok();
    let text = previous.clone().unwrap_or_default();
    let prev_reclaim = ini_get(&text, "experimental", "autoMemoryReclaim");
    let prev_sparse = ini_get(&text, "experimental", "sparseVhd");
    let written = ini_set(&text, "experimental", &[("autoMemoryReclaim", Some("dropCache")), ("sparseVhd", Some("true"))]);
    std::fs::write(&path, &written).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    Ok((
        Undo::Wsl { path: path.to_string_lossy().into(), previous, written, prev_reclaim, prev_sparse },
        "Wrote autoMemoryReclaim and sparseVhd to .wslconfig. Run wsl --shutdown (or restart) for it to take effect.".into(),
    ))
}

// ---------------------------------------------------------------- JSONC (editor settings)

const BEGIN: &str = "// dev-cleaner:begin";
const END: &str = "// dev-cleaner:end";

/// The text with comments blanked out byte for byte (same byte offsets),
/// so searches skip them.
fn blank_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    let mut in_str = false;
    // Comment bytes become spaces; multi-byte characters inside comments turn
    // into as many spaces as they had bytes, so offsets stay valid.
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                if b[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            for k in out.iter_mut().take((i + 2).min(b.len())).skip(i) {
                *k = b' ';
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Whether `"key":` appears outside comments.
pub(crate) fn jsonc_has_key(text: &str, key: &str) -> bool {
    let clean = blank_comments(text);
    let needle = format!("\"{key}\"");
    let mut from = 0;
    while let Some(i) = clean[from..].find(&needle) {
        let after = &clean[from + i + needle.len()..];
        if after.trim_start().starts_with(':') {
            return true;
        }
        from += i + needle.len();
    }
    false
}

/// Insert `entries` (`"key": value` without trailing commas) right after the
/// first `{`, between marker comments. None when the text has no object.
pub(crate) fn jsonc_insert(text: &str, entries: &[String]) -> Option<String> {
    if entries.is_empty() {
        return Some(text.to_string());
    }
    let clean = blank_comments(text);
    let brace = clean.find('{')?;
    let rest = clean[brace + 1..].trim_start();
    let empty = rest.starts_with('}');
    let mut block = format!("\n  {BEGIN} (Dev Cleaner › Responsiveness; undo from its History)\n");
    for (i, e) in entries.iter().enumerate() {
        let last = i + 1 == entries.len();
        block.push_str("  ");
        block.push_str(e);
        if !(last && empty) {
            block.push(',');
        }
        block.push('\n');
    }
    block.push_str("  ");
    block.push_str(END);
    let mut out = String::with_capacity(text.len() + block.len());
    out.push_str(&text[..brace + 1]);
    out.push_str(&block);
    if empty {
        out.push('\n');
    }
    out.push_str(&text[brace + 1..]);
    Some(out)
}

/// Remove the marked block (and the line break before it).
pub(crate) fn jsonc_remove(text: &str) -> Option<String> {
    let b = text.find(BEGIN)?;
    let e = text[b..].find(END)? + b + END.len();
    // Back up over the indentation and the newline before the marker.
    let mut start = b;
    while start > 0 && matches!(text.as_bytes()[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start > 0 && text.as_bytes()[start - 1] == b'\n' {
        start -= 1;
        if start > 0 && text.as_bytes()[start - 1] == b'\r' {
            start -= 1;
        }
    }
    let mut out = String::new();
    out.push_str(&text[..start]);
    out.push_str(&text[e..]);
    // An object that held only our block was `{\n}`: put back `{}` cleanly.
    Some(out)
}

const EDITORS: &[(&str, &str)] = &[("Code", "VS Code"), ("Code - Insiders", "VS Code Insiders"), ("Cursor", "Cursor"), ("Windsurf", "Windsurf"), ("VSCodium", "VSCodium")];

fn editor_settings() -> Vec<(String, PathBuf)> {
    EDITORS
        .iter()
        .filter_map(|(dir, name)| {
            let user = util::roaming_dir().join(dir).join("User");
            user.is_dir().then(|| (name.to_string(), user.join("settings.json")))
        })
        .collect()
}

fn editor_entries() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "files.watcherExclude",
            r#"{ "**/.git/objects/**": true, "**/.git/subtree-cache/**": true, "**/.hg/store/**": true, "**/node_modules/**": true, "**/target/**": true, "**/.next/**": true, "**/dist/**": true }"#,
        ),
        ("search.exclude", r#"{ "**/node_modules": true, "**/bower_components": true, "**/*.code-search": true, "**/target": true, "**/.next": true, "**/dist": true }"#),
        ("typescript.tsserver.maxTsServerMemory", "4096"),
    ]
}

fn missing_entries(text: &str) -> Vec<String> {
    editor_entries().into_iter().filter(|(k, _)| !jsonc_has_key(text, k)).map(|(k, v)| format!("\"{k}\": {v}")).collect()
}

fn editors_item() -> TuneItem {
    let eds = editor_settings();
    let pending: Vec<&String> = eds
        .iter()
        .filter(|(_, p)| !missing_entries(&std::fs::read_to_string(p).unwrap_or_else(|_| "{}".into())).is_empty())
        .map(|(n, _)| n)
        .collect();
    let names: Vec<&str> = eds.iter().map(|(n, _)| n.as_str()).collect();
    let state = if eds.is_empty() { "na" } else if pending.is_empty() { "applied" } else { "available" };
    TuneItem {
        id: "editors".into(),
        title: "Quieter editor file watching".into(),
        evidence: if eds.is_empty() {
            "No VS Code, Cursor or Windsurf settings found.".into()
        } else if pending.is_empty() {
            format!("{} already {} dependency and build folders.", names.join(", "), if names.len() == 1 { "skips" } else { "skip" })
        } else {
            format!(
                "{} {} every file in node_modules, target and dist, and the TypeScript server can grow without a cap.",
                pending.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
                if pending.len() == 1 { "watches" } else { "watch" }
            )
        },
        impact: 2,
        admin: false,
        note: "Reload editor windows".into(),
        state: state.into(),
        na_reason: eds.is_empty().then(|| "No VS Code-style editor found.".into()),
        detail: "Adds these keys to your user settings.json, only where you haven't set them yourself, between two marker comments. Your comments and other settings are kept. The editor's own defaults are included, so nothing they excluded starts being watched.".into(),
        code: editor_entries().iter().map(|(k, v)| format!("\"{k}\": {v}")).collect::<Vec<_>>().join(",\n"),
        link: None,
    }
}

fn editors_apply() -> R<(Undo, String)> {
    let mut files = vec![];
    let mut names = vec![];
    for (name, path) in editor_settings() {
        let previous = std::fs::read_to_string(&path).ok();
        let text = previous.clone().unwrap_or_else(|| "{\n}\n".into());
        let missing = missing_entries(&text);
        if missing.is_empty() {
            continue;
        }
        let Some(written) = jsonc_insert(&text, &missing) else { continue };
        std::fs::write(&path, &written).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        files.push(FileUndo { path: path.to_string_lossy().into(), previous, written });
        names.push(name);
    }
    if files.is_empty() {
        return Err("Every editor already has these settings.".into());
    }
    Ok((Undo::Editors { files }, format!("Updated settings for {}. Reload their windows.", names.join(", "))))
}

/// Put a file back: the old copy if nobody touched it since, otherwise just
/// take our block out.
fn undo_file(f: &FileUndo) -> R<String> {
    let path = Path::new(&f.path);
    let current = std::fs::read_to_string(path).ok();
    if current.as_deref() == Some(f.written.as_str()) {
        match &f.previous {
            Some(p) => std::fs::write(path, p).map_err(|e| e.to_string())?,
            None => std::fs::remove_file(path).map_err(|e| e.to_string())?,
        }
        return Ok("restored".into());
    }
    let Some(cur) = current else { return Ok("already gone".into()) };
    match jsonc_remove(&cur) {
        Some(t) => {
            std::fs::write(path, t).map_err(|e| e.to_string())?;
            Ok("removed our keys (the file changed since, so the rest was kept)".into())
        }
        None => Ok("our keys were already removed".into()),
    }
}

// ---------------------------------------------------------------- power plan

const HIGH: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
const ULTIMATE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

fn find_guid(line: &str) -> Option<String> {
    line.split(|c: char| !(c.is_ascii_hexdigit() || c == '-'))
        .find(|t| t.len() == 36 && t.matches('-').count() == 4)
        .map(|s| s.to_ascii_lowercase())
}

pub(crate) fn parse_powercfg(text: &str) -> Vec<Plan> {
    let mut v: Vec<Plan> = vec![];
    for line in text.lines() {
        let Some(guid) = find_guid(line) else { continue };
        let name = line.rsplit_once('(').and_then(|(_, r)| r.split_once(')')).map(|(n, _)| n.trim().to_string()).unwrap_or_default();
        let active = line.trim_end().ends_with('*');
        if let Some(p) = v.iter_mut().find(|p| p.guid == guid) {
            p.active |= active;
        } else {
            v.push(Plan { guid, name, active });
        }
    }
    v
}

fn is_fast(p: &Plan) -> bool {
    p.guid == HIGH || p.guid == ULTIMATE || p.name.to_ascii_lowercase().contains("ultimate")
}

fn plans() -> Vec<Plan> {
    if !cfg!(windows) || sandbox() {
        return vec![];
    }
    which("powercfg").and_then(|p| run_capture(&p, &["/list"], T)).map(|t| parse_powercfg(&t)).unwrap_or_default()
}

fn power_item() -> Option<TuneItem> {
    if !cfg!(windows) {
        return None;
    }
    let ps = plans();
    let active = ps.iter().find(|p| p.active);
    let target = ps.iter().find(|p| p.guid == HIGH).or_else(|| ps.iter().find(|p| is_fast(p)));
    let (state, evidence, na) = if sandbox() {
        ("available", "Test sandbox: the power plan isn't read.".to_string(), None)
    } else if active.is_some_and(is_fast) {
        ("applied", format!("The {} plan is active.", active.map(|a| a.name.as_str()).unwrap_or("")), None)
    } else if let Some(t) = target {
        ("available", format!("{} is active; {} keeps clocks up for builds.", active.map(|a| a.name.as_str()).unwrap_or("Another plan"), t.name), None)
    } else {
        (
            "na",
            format!("Only {} is available here (modern standby).", active.map(|a| a.name.as_str()).unwrap_or("Balanced")),
            Some("Windows offers only Balanced on this PC. Set Power mode to Best performance in Settings instead.".to_string()),
        )
    };
    Some(TuneItem {
        id: "power".into(),
        title: "Best performance power plan".into(),
        evidence,
        impact: 1,
        admin: false,
        note: "No restart".into(),
        state: state.into(),
        link: na.as_ref().map(|_| "ms-settings:powersleep".into()),
        na_reason: na,
        detail: "Keeps the CPU from dropping to low clocks between bursts, so builds and test runs start faster. More heat and fan noise; on a laptop, more battery use. Undo switches back to the plan you had.".into(),
        code: format!("powercfg /setactive {}", target.map(|t| t.guid.as_str()).unwrap_or(HIGH)),
    })
}

fn power_apply() -> R<(Undo, String)> {
    let ps = plans();
    let active = ps.iter().find(|p| p.active).ok_or("Could not read the active power plan.")?;
    let target = ps.iter().find(|p| p.guid == HIGH).or_else(|| ps.iter().find(|p| is_fast(p))).ok_or("No high performance plan on this PC.")?;
    let pc = which("powercfg").ok_or("powercfg not found")?;
    if !util::run_ok(&pc, &["/setactive", &target.guid], T) {
        return Err("powercfg refused the change.".into());
    }
    Ok((Undo::Power { previous: active.guid.clone(), previous_name: active.name.clone() }, format!("Switched from {} to {}.", active.name, target.name)))
}

// ---------------------------------------------------------------- animations

#[cfg(windows)]
mod spi {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, ANIMATIONINFO, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETANIMATION, SPI_GETCLIENTAREAANIMATION, SPI_SETANIMATION,
        SPI_SETCLIENTAREAANIMATION,
    };

    /// (client-area animations, minimize/maximize animation)
    pub fn get() -> Option<(bool, bool)> {
        // SAFETY: out-pointers to values we own, with the sizes the API expects.
        unsafe {
            let mut client: i32 = 0;
            if SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut client as *mut i32 as *mut _, 0) == 0 {
                return None;
            }
            let mut ai = ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: 0 };
            if SystemParametersInfoW(SPI_GETANIMATION, ai.cbSize, &mut ai as *mut ANIMATIONINFO as *mut _, 0) == 0 {
                return None;
            }
            Some((client != 0, ai.iMinAnimate != 0))
        }
    }

    pub fn set(client: bool, minimize: bool) -> bool {
        // SAFETY: SPI_SETCLIENTAREAANIMATION takes the BOOL as the pointer
        // value; SPI_SETANIMATION a pointer to an ANIMATIONINFO we own.
        unsafe {
            let flags = SPIF_UPDATEINIFILE | SPIF_SENDCHANGE;
            let a = SystemParametersInfoW(SPI_SETCLIENTAREAANIMATION, 0, client as usize as *mut _, flags) != 0;
            let mut ai = ANIMATIONINFO { cbSize: std::mem::size_of::<ANIMATIONINFO>() as u32, iMinAnimate: minimize as i32 };
            let b = SystemParametersInfoW(SPI_SETANIMATION, ai.cbSize, &mut ai as *mut ANIMATIONINFO as *mut _, flags) != 0;
            a && b
        }
    }
}

#[cfg(not(windows))]
mod spi {
    pub fn get() -> Option<(bool, bool)> {
        None
    }
    pub fn set(_: bool, _: bool) -> bool {
        false
    }
}

fn animation_item() -> Option<TuneItem> {
    if !cfg!(windows) {
        return None;
    }
    let cur = if sandbox() { Some((true, true)) } else { spi::get() };
    let applied = cur.is_some_and(|(c, m)| !c && !m);
    Some(TuneItem {
        id: "animations".into(),
        title: "Fewer animations".into(),
        evidence: if applied { "Window and menu animations are already off.".into() } else { "Windows animates every window open, close, minimize and menu.".into() },
        impact: 1,
        admin: false,
        note: "No restart".into(),
        state: if cur.is_none() { "na" } else if applied { "applied" } else { "available" }.into(),
        na_reason: cur.is_none().then(|| "Could not read the animation settings.".into()),
        detail: "Windows and menus appear at once. It feels snappier; it frees no memory and no CPU worth measuring. Same switch as Settings › Accessibility › Visual effects › Animation effects.".into(),
        code: "SystemParametersInfo(SPI_SETCLIENTAREAANIMATION, FALSE)\nSystemParametersInfo(SPI_SETANIMATION, iMinAnimate = 0)".into(),
        link: None,
    })
}

// ---------------------------------------------------------------- transparency

const PERSONALIZE: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

pub(crate) fn parse_reg_dword(out: &str, value: &str) -> Option<u32> {
    let line = out.lines().find(|l| l.split_whitespace().next().is_some_and(|w| w.eq_ignore_ascii_case(value)))?;
    let hex = line.split_whitespace().last()?;
    u32::from_str_radix(hex.trim_start_matches("0x"), 16).ok()
}

fn reg_dword(key: &str, value: &str) -> Option<u32> {
    let reg = which("reg")?;
    parse_reg_dword(&run_capture(&reg, &["query", key, "/v", value], T)?, value)
}

fn transparency_item() -> Option<TuneItem> {
    if !cfg!(windows) {
        return None;
    }
    let v = if sandbox() { Some(1) } else { reg_dword(PERSONALIZE, "EnableTransparency") };
    let applied = v == Some(0);
    Some(TuneItem {
        id: "transparency".into(),
        title: "Transparency effects off".into(),
        evidence: if applied { "Transparency is already off.".into() } else { "The taskbar, Start and window frames blur what's behind them.".into() },
        impact: 1,
        admin: false,
        note: "No restart".into(),
        state: if applied { "applied" } else { "available" }.into(),
        na_reason: None,
        detail: "Solid colours instead of the acrylic blur. A little less GPU work on every frame; mostly a matter of taste.".into(),
        code: format!("{PERSONALIZE}\nEnableTransparency = 0 (DWORD)"),
        link: None,
    })
}

fn transparency_apply() -> R<(Undo, String)> {
    let previous = reg_dword(PERSONALIZE, "EnableTransparency");
    let reg = which("reg").ok_or("reg not found")?;
    if !util::run_ok(&reg, &["add", PERSONALIZE, "/v", "EnableTransparency", "/t", "REG_DWORD", "/d", "0", "/f"], T) {
        return Err("Could not change the setting.".into());
    }
    Ok((Undo::Transparency { previous }, "Transparency effects turned off.".into()))
}

// ---------------------------------------------------------------- overview

fn good_chips() -> Vec<String> {
    if !cfg!(windows) || sandbox() {
        return vec![];
    }
    let mut v = vec![];
    let probe = crate::sysinfo_dev::windows_probe();
    if probe.compressed.is_some_and(|n| n > 0) {
        v.push("Memory compression on".into());
    }
    if !probe.page_files.is_empty() {
        v.push("Pagefile on".into());
    }
    if which("sc").and_then(|sc| run_capture(&sc, &["query", "SysMain"], T)).is_some_and(|o| o.contains("RUNNING")) {
        v.push("SysMain running".into());
    }
    if reg_dword(r"HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode") == Some(2) {
        v.push("GPU scheduling on".into());
    }
    v
}

fn overview() -> TuneOverview {
    let ((items, good), startup) = rayon::join(
        || {
            rayon::join(
                || {
                    let mut items: Vec<TuneItem> = vec![];
                    if cfg!(windows) {
                        items.push(wsl_item());
                    }
                    items.push(editors_item());
                    items.extend(power_item());
                    items.extend(animation_item());
                    items.extend(transparency_item());
                    items
                },
                good_chips,
            )
        },
        || crate::startup::list().iter().filter(|s| s.enabled).count() as u32,
    );
    TuneOverview { items, good, startup_enabled: Some(startup), dry_run: sandbox() }
}

fn title_of(id: &str) -> &'static str {
    match id {
        "wsl" => "Let WSL give memory back",
        "editors" => "Quieter editor file watching",
        "power" => "Best performance power plan",
        "animations" => "Fewer animations",
        "transparency" => "Transparency effects off",
        _ => "Unknown",
    }
}

fn apply_one(id: &str) -> R<(Undo, String)> {
    match id {
        "wsl" => wsl_apply(),
        "editors" => editors_apply(),
        "power" => power_apply(),
        "animations" => {
            let (client, minimize) = spi::get().ok_or("Could not read the animation settings.")?;
            if !spi::set(false, false) {
                return Err("Windows refused the change.".into());
            }
            Ok((Undo::Animation { client, minimize }, "Window and menu animations turned off.".into()))
        }
        "transparency" => transparency_apply(),
        _ => Err(format!("Unknown tweak {id}")),
    }
}

/// System tweaks that never run in the test sandbox.
fn system_tweak(id: &str) -> bool {
    matches!(id, "power" | "animations" | "transparency")
}

fn apply(app: &AppHandle, ids: &[String]) -> Vec<TuneOutcome> {
    let mut journal = load_journal(app);
    let mut out = vec![];
    for id in ids {
        if sandbox() && system_tweak(id) {
            out.push(TuneOutcome { id: id.clone(), ok: true, message: "Test sandbox: nothing was changed.".into(), entry_id: None, dry_run: true });
            continue;
        }
        match apply_one(id) {
            Ok((undo, summary)) => {
                let entry_id = format!("{}-{}", now_secs(), id);
                journal.push(JournalEntry {
                    id: entry_id.clone(),
                    tweak: id.clone(),
                    title: title_of(id).into(),
                    summary: summary.clone(),
                    applied_at: now_secs(),
                    undone_at: None,
                    undo_note: None,
                    undo,
                });
                out.push(TuneOutcome { id: id.clone(), ok: true, message: summary, entry_id: Some(entry_id), dry_run: false });
            }
            Err(e) => out.push(TuneOutcome { id: id.clone(), ok: false, message: e, entry_id: None, dry_run: false }),
        }
    }
    if let Err(e) = save_journal(app, &journal) {
        for o in out.iter_mut().filter(|o| o.ok && !o.dry_run) {
            o.message.push_str(&format!(" (History could not be saved: {e})"));
        }
    }
    out
}

fn undo_entry(u: &Undo) -> R<String> {
    match u {
        Undo::Wsl { path, previous, written, prev_reclaim, prev_sparse } => {
            let p = Path::new(path);
            let cur = std::fs::read_to_string(p).ok();
            if cur.as_deref() == Some(written.as_str()) {
                match previous {
                    Some(t) => std::fs::write(p, t).map_err(|e| e.to_string())?,
                    None => std::fs::remove_file(p).map_err(|e| e.to_string())?,
                }
                Ok(".wslconfig restored. Run wsl --shutdown for it to take effect.".into())
            } else {
                let cur = cur.unwrap_or_default();
                let t = ini_set(&cur, "experimental", &[("autoMemoryReclaim", prev_reclaim.as_deref()), ("sparseVhd", prev_sparse.as_deref())]);
                std::fs::write(p, t).map_err(|e| e.to_string())?;
                Ok("Put back the two .wslconfig keys; the rest of the file changed since and was kept.".into())
            }
        }
        Undo::Editors { files } => {
            let notes: Vec<String> = files.iter().map(|f| undo_file(f).map(|n| format!("{}: {n}", f.path))).collect::<R<_>>()?;
            Ok(format!("Editor settings put back. {}", notes.join("; ")))
        }
        Undo::Power { previous, previous_name } => {
            if sandbox() {
                return Ok("Test sandbox: nothing was changed.".into());
            }
            let pc = which("powercfg").ok_or("powercfg not found")?;
            if !util::run_ok(&pc, &["/setactive", previous], T) {
                return Err("powercfg refused the change.".into());
            }
            Ok(format!("Switched back to {previous_name}."))
        }
        Undo::Animation { client, minimize } => {
            if sandbox() {
                return Ok("Test sandbox: nothing was changed.".into());
            }
            if !spi::set(*client, *minimize) {
                return Err("Windows refused the change.".into());
            }
            Ok("Animations put back.".into())
        }
        Undo::Transparency { previous } => {
            if sandbox() {
                return Ok("Test sandbox: nothing was changed.".into());
            }
            let reg = which("reg").ok_or("reg not found")?;
            let ok = match previous {
                Some(v) => util::run_ok(&reg, &["add", PERSONALIZE, "/v", "EnableTransparency", "/t", "REG_DWORD", "/d", &v.to_string(), "/f"], T),
                None => util::run_ok(&reg, &["delete", PERSONALIZE, "/v", "EnableTransparency", "/f"], T),
            };
            if !ok {
                return Err("Could not change the setting.".into());
            }
            Ok("Transparency effects put back.".into())
        }
    }
}

fn undo(app: &AppHandle, entry_id: &str) -> R<TuneOutcome> {
    let mut journal = load_journal(app);
    let e = journal.iter_mut().find(|e| e.id == entry_id).ok_or("No such change in History.")?;
    if e.undone_at.is_some() {
        return Err("Already undone.".into());
    }
    let msg = undo_entry(&e.undo)?;
    e.undone_at = Some(now_secs());
    e.undo_note = Some(msg.clone());
    let tweak = e.tweak.clone();
    save_journal(app, &journal)?;
    Ok(TuneOutcome { id: tweak, ok: true, message: msg, entry_id: Some(entry_id.to_string()), dry_run: false })
}

// ---------------------------------------------------------------- commands

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tune_list() -> R<TuneOverview> {
    blocking(overview).await
}

#[tauri::command]
pub async fn tune_apply(app: AppHandle, ids: Vec<String>) -> R<Vec<TuneOutcome>> {
    blocking(move || apply(&app, &ids)).await
}

#[tauri::command]
pub async fn tune_undo(app: AppHandle, entry_id: String) -> R<TuneOutcome> {
    blocking(move || undo(&app, &entry_id)).await?
}

#[tauri::command]
pub async fn tune_history(app: AppHandle) -> R<Vec<JournalEntry>> {
    blocking(move || {
        let mut j = load_journal(&app);
        j.reverse();
        j
    })
    .await
}

#[cfg(test)]
pub(crate) fn tests_overview() -> String {
    format!("{:#?}", overview())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wslconfig_merge_keeps_other_settings() {
        let src = "[wsl2]\nmemory=16GB\n# keep me\nprocessors=8\n\n[experimental]\nautoMemoryReclaim=disabled\nnetworkingMode=mirrored\n";
        let out = ini_set(src, "experimental", &[("autoMemoryReclaim", Some("dropCache")), ("sparseVhd", Some("true"))]);
        assert!(out.contains("memory=16GB") && out.contains("# keep me") && out.contains("networkingMode=mirrored"));
        assert_eq!(ini_get(&out, "experimental", "autoMemoryReclaim").as_deref(), Some("dropCache"));
        assert_eq!(ini_get(&out, "experimental", "sparseVhd").as_deref(), Some("true"));
        assert!(!out.contains("disabled"));
        assert!(wsl_reclaim_on(&out));
        // Undo the keys: the old value comes back, the new key goes.
        let back = ini_set(&out, "experimental", &[("autoMemoryReclaim", Some("disabled")), ("sparseVhd", None)]);
        assert_eq!(ini_get(&back, "experimental", "autoMemoryReclaim").as_deref(), Some("disabled"));
        assert!(ini_get(&back, "experimental", "sparseVhd").is_none());
        assert!(back.contains("processors=8"));
    }

    #[test]
    fn wslconfig_section_is_added_when_missing() {
        let out = ini_set("[wsl2]\nmemory=8GB\n", "experimental", &[("autoMemoryReclaim", Some("dropCache"))]);
        assert_eq!(out, "[wsl2]\nmemory=8GB\n\n[experimental]\nautoMemoryReclaim=dropCache\n");
        assert_eq!(ini_set("", "experimental", &[("sparseVhd", Some("true"))]), "[experimental]\nsparseVhd=true\n");
        // Keys go into the existing section, before the next one.
        let mid = ini_set("[experimental]\nfoo=1\n\n[wsl2]\nmemory=8GB\n", "experimental", &[("sparseVhd", Some("true"))]);
        assert!(mid.find("sparseVhd").unwrap() < mid.find("[wsl2]").unwrap());
    }

    #[test]
    fn jsonc_insert_keeps_comments_and_skips_commented_keys() {
        let src = "// my settings\n{\n  // \"files.watcherExclude\": {},\n  \"editor.fontSize\": 14, /* big */\n  \"url\": \"http://x//y\"\n}\n";
        assert!(!jsonc_has_key(src, "files.watcherExclude"), "a commented-out key doesn't count");
        assert!(jsonc_has_key(src, "editor.fontSize"));
        let out = jsonc_insert(src, &missing_entries(src)).unwrap();
        assert!(out.starts_with("// my settings\n{\n  // dev-cleaner:begin"));
        assert!(out.contains("/* big */") && out.contains("http://x//y"));
        assert!(jsonc_has_key(&out, "files.watcherExclude") && jsonc_has_key(&out, "typescript.tsserver.maxTsServerMemory"));
        assert!(out.contains("\"typescript.tsserver.maxTsServerMemory\": 4096,\n"), "followed by existing keys, so a comma");
        assert_eq!(jsonc_remove(&out).unwrap(), src);
    }

    #[test]
    fn jsonc_insert_into_an_empty_object_has_no_trailing_comma() {
        let out = jsonc_insert("{}", &["\"a\": 1".into(), "\"b\": 2".into()]).unwrap();
        assert!(out.contains("\"a\": 1,\n  \"b\": 2\n"));
        assert!(out.ends_with("// dev-cleaner:end\n}"));
        let back = jsonc_remove(&out).unwrap();
        assert_eq!(back.replace('\n', ""), "{}");
        // Keys the user already set are left alone.
        let mine = "{ \"search.exclude\": { \"x\": true } }";
        assert!(!missing_entries(mine).iter().any(|e| e.starts_with("\"search.exclude\"")));
    }

    #[test]
    fn parses_powercfg_list() {
        let t = "Existing Power Schemes (* Active)\n-----------------------------------\nPower Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced)\nPower Scheme GUID: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (High performance) *\nPower Scheme GUID: e5d34f7d-1009-4aa6-877f-d6b132374008  (Ultimate Performance)\n";
        let p = parse_powercfg(t);
        assert_eq!(p.len(), 3);
        assert!(p[1].active && p[1].guid == HIGH);
        assert!(is_fast(&p[2]));
        assert!(!is_fast(&p[0]));
    }

    #[test]
    fn parses_reg_dwords() {
        let out = "\nHKEY_CURRENT_USER\\...\\Personalize\n    EnableTransparency    REG_DWORD    0x1\n";
        assert_eq!(parse_reg_dword(out, "EnableTransparency"), Some(1));
        assert_eq!(parse_reg_dword("ERROR: not found", "EnableTransparency"), None);
    }
}

