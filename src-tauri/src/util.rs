//! Small helpers shared by every module: running tools without a console
//! window, finding them on PATH, and measuring or emptying folders.

use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Running inside a Flatpak sandbox. Tools (npm, docker, ps, pkexec) live on
/// the host there, so commands go through `flatpak-spawn --host`, and
/// processes and ports are read from the host.
pub fn in_flatpak() -> bool {
    cfg!(target_os = "linux") && (std::env::var_os("FLATPAK_ID").is_some() || Path::new("/.flatpak-info").exists())
}

/// A `Command` that never flashes a console window on Windows, and runs on
/// the host when inside a Flatpak.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = if in_flatpak() {
        let mut h = Command::new("flatpak-spawn");
        h.arg("--host").arg(program);
        h
    } else {
        Command::new(program)
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Find an executable on PATH, honouring PATHEXT on Windows so `npm` resolves
/// to `npm.cmd`.
pub fn which(name: &str) -> Option<PathBuf> {
    if in_flatpak() {
        // The sandbox's PATH holds only the runtime; ask the host's shell.
        let out = Command::new("flatpak-spawn")
            .args(["--host", "sh", "-c", "command -v \"$1\"", "sh", name])
            .stdin(Stdio::null())
            .output()
            .ok()?;
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        return (out.status.success() && p.starts_with('/')).then(|| PathBuf::from(p));
    }
    let path = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let exts: Vec<String> = std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        {
            for ext in &exts {
                let p = dir.join(format!("{name}{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Run a program and return trimmed stdout (falling back to stderr), or None
/// if it is missing, fails, or takes longer than `timeout`.
pub fn run_capture(program: &Path, args: &[&str], timeout: Duration) -> Option<String> {
    let child = command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let (tx, rx) = mpsc::channel();
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => {
            let text = if out.stdout.is_empty() { out.stderr } else { out.stdout };
            Some(decode(&text).trim().to_string())
        }
        _ => {
            kill_pid(pid);
            None
        }
    }
}

/// Run a program to completion and report whether it exited cleanly.
pub fn run_ok(program: &Path, args: &[&str], timeout: Duration) -> bool {
    let Ok(mut child) = command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

fn kill_pid(pid: u32) {
    let mut sys = sysinfo::System::new();
    let p = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
    if let Some(proc_) = sys.process(p) {
        proc_.kill();
    }
}

/// Some Windows tools (wsl.exe) write UTF-16LE. Detect it by the NUL bytes.
pub fn decode(bytes: &[u8]) -> String {
    let nuls = bytes.iter().filter(|b| **b == 0).count();
    if bytes.len() >= 4 && nuls * 3 > bytes.len() {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&units).replace('\u{feff}', "")
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Size in bytes and file count of everything under `path`. Symlinks are
/// counted as themselves and never followed.
pub fn dir_size(path: &Path) -> (u64, u64) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, 0);
    };
    if !meta.is_dir() {
        return (meta.len(), 1);
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, 0);
    };
    let entries: Vec<_> = entries.flatten().collect();
    entries
        .par_iter()
        .map(|e| match e.file_type() {
            Ok(ft) if ft.is_dir() => dir_size(&e.path()),
            Ok(_) => (e.metadata().map(|m| m.len()).unwrap_or(0), 1),
            Err(_) => (0, 0),
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

/// Remove a folder tree. Read-only files (Go's module cache marks every file
/// read-only) are made writable and the removal retried once.
pub fn remove_tree(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(first) => {
            clear_readonly(path);
            std::fs::remove_dir_all(path).map_err(|_| first)
        }
    }
}

fn clear_readonly(path: &Path) {
    if let Ok(entries) = std::fs::read_dir(path) {
        for e in entries.flatten() {
            let p = e.path();
            if let Ok(meta) = std::fs::symlink_metadata(&p) {
                if meta.is_dir() {
                    clear_readonly(&p);
                }
                let mut perm = meta.permissions();
                if perm.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(&p, perm);
                }
            }
        }
    }
}

/// Empty a folder but keep the folder itself. Entries newer than `min_age`
/// are left alone, and anything in use is skipped. Returns (removed, skipped).
pub fn empty_dir(path: &Path, min_age: Option<Duration>) -> (u64, u64) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, 0);
    };
    let now = std::time::SystemTime::now();
    let entries: Vec<_> = entries.flatten().collect();
    entries
        .par_iter()
        .map(|e| {
            let p = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                return (0, 1);
            };
            if let Some(age) = min_age {
                let young = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .is_some_and(|d| d < age);
                if young {
                    return (0, 1);
                }
            }
            let res = if meta.is_dir() {
                remove_tree(&p)
            } else {
                std::fs::remove_file(&p).or_else(|_| {
                    let mut perm = meta.permissions();
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = std::fs::set_permissions(&p, perm);
                    std::fs::remove_file(&p)
                })
            };
            if res.is_ok() { (1, 0) } else { (0, 1) }
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
}

/// Pull the first version-looking token (digits and dots) out of tool output.
pub fn version_token(text: &str) -> Option<String> {
    for raw in text.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')') {
        let t = raw.trim_start_matches(['v', 'V']);
        let t = t.trim_start_matches("go");
        if t.chars().next().is_some_and(|c| c.is_ascii_digit())
            && t.contains('.')
            && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '+')
        {
            return Some(t.to_string());
        }
    }
    None
}

/// The end-to-end suite points every cache path into a throwaway folder with
/// this variable, so a test can never clean a real cache.
pub fn fake_home() -> Option<PathBuf> {
    std::env::var_os("DEV_CLEANER_FAKE_HOME").map(PathBuf::from)
}

pub fn home() -> PathBuf {
    fake_home().or_else(dirs::home_dir).unwrap_or_else(|| PathBuf::from("."))
}

/// %LOCALAPPDATA% on Windows, ~/Library/Application Support on macOS.
pub fn local_dir() -> PathBuf {
    match fake_home() {
        Some(f) => f.join("local"),
        // Flatpak points XDG dirs into the sandbox; the caches we clean are
        // the host's.
        None if in_flatpak() => home().join(".local").join("share"),
        None => dirs::data_local_dir().unwrap_or_else(home),
    }
}

/// %LOCALAPPDATA% on Windows, ~/Library/Caches on macOS.
pub fn cache_dir() -> PathBuf {
    match fake_home() {
        Some(f) => f.join("cache"),
        None if in_flatpak() => home().join(".cache"),
        None => dirs::cache_dir().unwrap_or_else(|| home().join(".cache")),
    }
}

/// %APPDATA% (Roaming) on Windows, ~/Library/Application Support on macOS:
/// where VS Code, Cursor, JetBrains and friends keep user settings.
pub fn roaming_dir() -> PathBuf {
    match fake_home() {
        Some(f) => f.join("roaming"),
        None if in_flatpak() => home().join(".config"),
        None => dirs::config_dir().unwrap_or_else(|| home().join(".config")),
    }
}

/// Portable mode: a file named `portable` next to the exe keeps every bit of
/// app data in a `data` folder beside it (Scoop installs it this way).
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join("portable").is_file().then(|| dir.join("data"))
}

pub fn temp_dir() -> PathBuf {
    match fake_home() {
        Some(f) => f.join("temp"),
        None => std::env::temp_dir(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_versions_in_common_outputs() {
        assert_eq!(version_token("v22.22.0").as_deref(), Some("22.22.0"));
        assert_eq!(version_token("rustc 1.99.1 (58cf8f9c3 2026-10-13)").as_deref(), Some("1.99.1"));
        assert_eq!(version_token("go version go1.27.0 windows/amd64").as_deref(), Some("1.27.0"));
        assert_eq!(version_token("Docker version 29.8.2, build 7fc2dff").as_deref(), Some("29.8.2"));
        assert_eq!(version_token("Python 3.13.5").as_deref(), Some("3.13.5"));
        assert_eq!(version_token("nothing here"), None);
    }

    #[test]
    fn decodes_utf16_output() {
        let s: Vec<u8> = "Ubuntu".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode(&s), "Ubuntu");
    }

    #[test]
    fn measures_and_empties_a_folder() {
        let dir = std::env::temp_dir().join(format!("dc-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("a/b/f.txt"), b"hello").unwrap();
        std::fs::write(dir.join("g.txt"), b"abc").unwrap();
        assert_eq!(dir_size(&dir), (8, 2));
        let (removed, skipped) = empty_dir(&dir, None);
        assert_eq!((removed, skipped), (2, 0));
        assert!(dir.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
