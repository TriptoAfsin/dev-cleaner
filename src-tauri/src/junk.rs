//! The system-junk engine: the drive walk (unfinished downloads, Thumbs.db,
//! Steam shader caches), the Recycle Bin / Trash, and the single elevated
//! script admin cleanups run in. What to clean lives in
//! definitions/caches.json; this file only does what needs code.
//!
//! What is deliberately NOT cleaned, because it does harm: Windows Prefetch
//! (Windows trims it and launches get slower), WinSxS by hand,
//! pagefile/hiberfil, macOS language files and app thinning (break code
//! signatures), the macOS sleep image, browser profile data other than its
//! caches (cookies, history, logins), and package-manager databases.

use crate::caches::{which, How, Target};
use crate::util::{self, home};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Admin steps a definition can name: (before, command, after). Definitions
/// never carry commands of their own; these are the only ones that run
/// elevated.
pub(crate) fn admin_action(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match name {
        "stop-update-services" => ("Stop-Service wuauserv,bits -Force", "", "Start-Service wuauserv,bits"),
        "delivery-optimization" => ("", "Delete-DeliveryOptimizationCache -Force", ""),
        "dism-component-cleanup" => ("", "Dism.exe /Online /Cleanup-Image /StartComponentCleanup", ""),
        "cleanmgr-autoclean" => ("", "cleanmgr.exe /autoclean", ""),
        "thin-time-machine-snapshots" => ("", "/usr/bin/tmutil thinlocalsnapshots / 999999999999 4", ""),
        "journal-vacuum" => ("", "journalctl --vacuum-time=7d", ""),
        "apt-clean" => ("", "apt-get clean", ""),
        "dnf-clean" => ("", "dnf clean all", ""),
        "paccache" => ("", "paccache -rk2", ""),
        "snap-remove-disabled" => (
            "",
            "snap list --all | awk '/disabled/{print $1, $3}' | while read n r; do snap remove \"$n\" --revision=\"$r\"; done",
            "",
        ),
        _ => return None,
    })
}

/// Files in `dir` whose names match one of the extensions (no dot).
pub(crate) fn files_with(dir: &Path, exts: &[&str], prefix: Option<&[&str]>) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    rd.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            let ext_ok = exts.is_empty() || exts.iter().any(|x| name.ends_with(&format!(".{x}")));
            let pre_ok = prefix.is_none_or(|ps| ps.iter().any(|x| name.starts_with(x)));
            ext_ok && pre_ok
        })
        .collect()
}

/// Cache folders of every Chromium profile under a "User Data" folder: only
/// the caches, never cookies, history, logins or extensions.
pub(crate) fn chromium(user_data: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(user_data)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n == "Default" || n.starts_with("Profile ") || n == "Guest Profile"
        })
        .flat_map(|e| ["Cache", "Code Cache", "GPUCache", "Service Worker/CacheStorage"].iter().map(move |t| e.path().join(t)).collect::<Vec<_>>())
        .collect();
    v.extend(["ShaderCache", "GrShaderCache", "GraphiteDawnCache", "component_crx_cache"].iter().map(|d| user_data.join(d)));
    v
}

// ------------------------------------------------------------ drive walk

/// What a walk of the chosen drives found. Kept between a scan and the
/// clean that follows it, so cleaning doesn't walk every drive again.
#[derive(Default, Clone)]
pub(crate) struct DriveJunk {
    pub partials: Vec<PathBuf>,
    pub thumbs: Vec<PathBuf>,
    pub shader_caches: Vec<PathBuf>,
}

static LAST_WALK: std::sync::Mutex<Option<DriveJunk>> = std::sync::Mutex::new(None);

const PARTIAL_EXT: &[&str] = &["crdownload", "part", "partial", "download", "opdownload"];

/// Folders never worth walking for junk: the OS, other users, caches the
/// other pages measure, version control and dependency trees.
const WALK_SKIP: &[&str] = &[
    "windows", "$recycle.bin", "system volume information", "programdata", "recovery", "$windows.~bt", "$windows.~ws",
    "program files", "program files (x86)", "windowsapps", "xboxgames", "epic games",
    "appdata", "library", "node_modules", ".git", ".hg", ".svn", "target", ".venv", "venv", "site-packages",
    ".cache", ".cargo", ".rustup", ".npm", ".pnpm-store", "proc", "sys", "dev", "snap", "private", "system",
];

fn walk_drive(dir: &Path, depth: u32, out: &std::sync::Mutex<DriveJunk>) {
    use rayon::prelude::*;
    if depth > 7 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut subdirs = vec![];
    let mut found = DriveJunk::default();
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        if ft.is_dir() {
            // Steam keeps per-game shader caches next to each library.
            let in_steamapps = dir.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("steamapps"));
            if name == "shadercache" && in_steamapps {
                found.shader_caches.push(e.path());
            } else if in_steamapps && name == "common" {
                // Installed games: their files belong to the game. Removing
                // even a Thumbs.db makes Steam's "verify files" fetch it again.
                continue;
            } else if !WALK_SKIP.contains(&name.as_str()) && !name.starts_with('$') {
                subdirs.push(e.path());
            }
        } else if name == "thumbs.db" {
            found.thumbs.push(e.path());
        } else if PARTIAL_EXT.iter().any(|x| name.ends_with(&format!(".{x}"))) {
            found.partials.push(e.path());
        }
    }
    if !(found.partials.is_empty() && found.thumbs.is_empty() && found.shader_caches.is_empty()) {
        if let Ok(mut o) = out.lock() {
            o.partials.extend(found.partials);
            o.thumbs.extend(found.thumbs);
            o.shader_caches.extend(found.shader_caches);
        }
    }
    subdirs.par_iter().for_each(|d| walk_drive(d, depth + 1, out));
}

/// Walk every chosen drive (all fixed drives when none are chosen). Never
/// under the e2e fake home: a test must not find files on real drives.
pub(crate) fn drive_junk(drives: Option<&[String]>, walk: bool) -> DriveJunk {
    use rayon::prelude::*;
    if util::fake_home().is_some() {
        return DriveJunk::default();
    }
    if !walk {
        if let Some(j) = LAST_WALK.lock().ok().and_then(|w| w.clone()) {
            return j;
        }
    }
    let roots: Vec<PathBuf> = sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|d| !d.is_removable() && d.total_space() > 0)
        .map(|d| d.mount_point().to_path_buf())
        .filter(|m| drive_chosen(m, drives))
        // On macOS and Linux "/" holds the system; walk the home folder instead.
        .map(|m| if !cfg!(windows) && m == Path::new("/") { home() } else { m })
        .filter(|m| !(cfg!(target_os = "macos") && m.starts_with("/System")))
        .collect();
    let out = std::sync::Mutex::new(DriveJunk::default());
    roots.par_iter().for_each(|r| walk_drive(r, 0, &out));
    // Program Files is skipped by the walk, but Steam's default library lives
    // there and its shader cache is still worth listing.
    if cfg!(windows) {
        let pf86 = std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into());
        let sc = PathBuf::from(pf86).join("Steam").join("steamapps").join("shadercache");
        let on_chosen = sc.components().next().is_some_and(|c| drive_chosen(Path::new(&format!("{}\\", c.as_os_str().to_string_lossy())), drives));
        if sc.is_dir() && on_chosen {
            if let Ok(mut o) = out.lock() {
                o.shader_caches.push(sc);
            }
        }
    }
    let j = out.into_inner().unwrap_or_default();
    if let Ok(mut w) = LAST_WALK.lock() {
        *w = Some(j.clone());
    }
    j
}

/// Whether a drive's mount point is among the chosen ones (None = all).
pub(crate) fn drive_chosen(mount: &Path, drives: Option<&[String]>) -> bool {
    let key = |s: &str| s.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    let m = key(&mount.to_string_lossy());
    drives.is_none_or(|ds| ds.iter().any(|d| key(d) == m))
}

// ------------------------------------------------------------ recycle bin

/// Bytes and items in the Recycle Bin / Trash, or None if it can't be read.
pub fn recycle_bin_size() -> Option<(u64, u64)> {
    if let Some(f) = util::fake_home() {
        let (s, n) = util::dir_size(&f.join("trash"));
        return Some((s, n));
    }
    os_recycle_size()
}

#[cfg(windows)]
fn os_recycle_size() -> Option<(u64, u64)> {
    use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    // A NULL root is rejected for queries, so ask each fixed drive.
    let mut total = (0u64, 0u64);
    let mut any = false;
    let chosen = crate::caches::chosen_drives();
    for d in sysinfo::Disks::new_with_refreshed_list().list().iter().filter(|d| drive_chosen(d.mount_point(), chosen.as_deref())) {
        let root: Vec<u16> = d.mount_point().as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, i64Size: 0, i64NumItems: 0 };
        // SAFETY: root is NUL-terminated and info is a correctly sized struct we own.
        if unsafe { SHQueryRecycleBinW(root.as_ptr(), &mut info) } == 0 {
            any = true;
            total.0 += info.i64Size.max(0) as u64;
            total.1 += info.i64NumItems.max(0) as u64;
        }
    }
    any.then_some(total)
}

#[cfg(target_os = "macos")]
fn os_recycle_size() -> Option<(u64, u64)> {
    let t = home().join(".Trash");
    t.is_dir().then(|| util::dir_size(&t))
}

#[cfg(target_os = "linux")]
fn os_recycle_size() -> Option<(u64, u64)> {
    let t = home().join(".local/share/Trash/files");
    t.is_dir().then(|| util::dir_size(&t))
}

pub fn empty_recycle_bin() -> bool {
    if let Some(f) = util::fake_home() {
        let dir = f.join("trash");
        return util::empty_dir(&dir, None).1 == 0;
    }
    os_empty_recycle()
}

#[cfg(windows)]
fn os_empty_recycle() -> bool {
    use windows_sys::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};
    // Each chosen drive has its own bin; empty only those.
    let chosen = crate::caches::chosen_drives();
    let mut ok = true;
    for d in sysinfo::Disks::new_with_refreshed_list().list().iter().filter(|d| !d.is_removable() && drive_chosen(d.mount_point(), chosen.as_deref())) {
        let root: Vec<u16> = d.mount_point().as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        // SAFETY: a NULL window and a NUL-terminated drive root.
        let hr = unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), root.as_ptr(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) };
        // S_OK, or an already-empty bin reported as E_UNEXPECTED.
        ok &= hr == 0 || hr as u32 == 0x8000_FFFF;
    }
    ok
}

#[cfg(target_os = "macos")]
fn os_empty_recycle() -> bool {
    // Finder empties the Trash on every volume; asking it also avoids the
    // Full Disk Access error deleting ~/.Trash directly can hit.
    util::run_ok(Path::new("/usr/bin/osascript"), &["-e", "tell application \"Finder\" to empty trash"], Duration::from_secs(120))
        || util::empty_dir(&home().join(".Trash"), None).1 == 0
}

#[cfg(target_os = "linux")]
fn os_empty_recycle() -> bool {
    let base = home().join(".local/share/Trash");
    let a = util::empty_dir(&base.join("files"), None).1;
    let b = util::empty_dir(&base.join("info"), None).1;
    a == 0 && b == 0
}

// ------------------------------------------------------------- elevation

pub fn admin_available() -> bool {
    util::fake_home().is_none() && (cfg!(any(windows, target_os = "macos")) || which("pkexec").is_some())
}

pub(crate) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub(crate) fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// One script for every chosen admin item, in the OS's shell.
pub(crate) fn admin_script(items: &[&Target]) -> String {
    let mut lines = vec![];
    if cfg!(windows) {
        lines.push("$ErrorActionPreference = 'SilentlyContinue'".to_string());
    }
    for t in items {
        let How::Admin { days, pre, cmd, post } = &t.how else { continue };
        if !pre.is_empty() {
            lines.push(pre.to_string());
        }
        if !cmd.is_empty() {
            lines.push(cmd.to_string());
        } else {
            for p in &t.paths {
                let path = p.to_string_lossy();
                if cfg!(windows) {
                    let filter = days.map(|d| format!(" | Where-Object {{ $_.LastWriteTime -lt (Get-Date).AddDays(-{d}) }}")).unwrap_or_default();
                    lines.push(format!(
                        "$i = Get-Item -LiteralPath {q} -Force; if ($i) {{ if ($i.PSIsContainer) {{ $x = Get-ChildItem -LiteralPath {q} -Force }} else {{ $x = @($i) }}; $x{filter} | Remove-Item -Recurse -Force }}",
                        q = ps_quote(&path)
                    ));
                } else {
                    let q = sh_quote(&path);
                    lines.push(match days {
                        Some(d) => format!("[ -e {q} ] && find {q} -type f -mtime +{d} -delete"),
                        None => format!("[ -d {q} ] && find {q} -mindepth 1 -maxdepth 1 -exec rm -rf {{}} +"),
                    });
                }
            }
        }
        if !post.is_empty() {
            lines.push(post.to_string());
        }
    }
    lines.push("exit 0".into());
    lines.join("\n")
}

/// Run every chosen admin item in one elevated script: one UAC prompt on
/// Windows, one password prompt on macOS (osascript) and Linux (pkexec).
/// Returns false if the user cancelled or elevation failed.
pub(crate) fn run_admin(items: &[&Target]) -> bool {
    if util::fake_home().is_some() || items.is_empty() {
        return false;
    }
    run_elevated(&admin_script(items))
}

/// Run a script with admin rights: PowerShell through UAC on Windows (or
/// directly when already elevated), sh through the macOS password prompt,
/// sh through pkexec on Linux. One prompt per call. False if cancelled or
/// it failed. Never under the e2e fake home.
pub(crate) fn run_elevated(script: &str) -> bool {
    if util::fake_home().is_some() {
        return false;
    }
    let script = script.to_string();
    let long = Duration::from_secs(1800);
    if cfg!(windows) {
        let path = std::env::temp_dir().join(format!("dev-cleaner-admin-{}.ps1", std::process::id()));
        if std::fs::write(&path, &script).is_err() {
            return false;
        }
        let Some(ps) = which("powershell") else { return false };
        let file = path.to_string_lossy().to_string();
        let ok = if crate::sysinfo_dev::is_admin() {
            util::run_ok(&ps, &["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &file], long)
        } else {
            let launch = format!(
                "$p = Start-Process powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',{}; exit $p.ExitCode",
                ps_quote(&file)
            );
            util::run_ok(&ps, &["-NoProfile", "-Command", &launch], long)
        };
        let _ = std::fs::remove_file(&path);
        ok
    } else if cfg!(target_os = "macos") {
        let escaped = script.replace('\\', "\\\\").replace('"', "\\\"");
        let apple = format!("do shell script \"{escaped}\" with administrator privileges");
        util::run_ok(Path::new("/usr/bin/osascript"), &["-e", &apple], long)
    } else {
        which("pkexec").is_some_and(|pk| util::run_ok(&pk, &["sh", "-c", &script], long))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_script_filters_by_age_and_quotes_paths() {
        let t = |id, paths, how| Target { id, name: id, eco: id, about: id, risk: crate::caches::Risk::Safe, junk: true, paths, how, ask: &[], before: None };
        let t1 = t("x", vec![PathBuf::from(if cfg!(windows) { r"C:\Win dows\Te'mp" } else { "/var/lo'g" })], How::Admin { days: Some(7), pre: "", cmd: "", post: "" });
        let t2 = t("y", vec![], How::Admin { days: None, pre: "STOP", cmd: "RUN", post: "START" });
        let s = admin_script(&[&t1, &t2]);
        if cfg!(windows) {
            assert!(s.contains("AddDays(-7)"));
            assert!(s.contains("'C:\\Win dows\\Te''mp'"));
        } else {
            assert!(s.contains("-mtime +7"));
            assert!(s.contains("'/var/lo'\\''g'"));
        }
        let (a, b, c) = (s.find("STOP").unwrap(), s.find("RUN").unwrap(), s.find("START").unwrap());
        assert!(a < b && b < c, "services stop, command runs, services start");
    }

    #[test]
    fn chromium_lists_only_cache_folders() {
        let root = std::env::temp_dir().join(format!("dc-chrome-{}", std::process::id()));
        for p in ["Default", "Profile 2", "System Profile", "Crashpad"] {
            std::fs::create_dir_all(root.join(p)).unwrap();
        }
        let v = chromium(&root);
        assert!(v.iter().any(|p| p.ends_with("Default/Cache") || p.ends_with("Default\\Cache")));
        assert!(v.iter().any(|p| p.to_string_lossy().contains("Profile 2")));
        assert!(!v.iter().any(|p| p.to_string_lossy().contains("System Profile")));
        assert!(!v.iter().any(|p| p.to_string_lossy().contains("Cookies") || p.to_string_lossy().contains("History")));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
