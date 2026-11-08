//! System junk: OS temp folders, update leftovers, error reports, logs,
//! thumbnail caches, browser and chat-app caches, the Recycle Bin / Trash,
//! and OS maintenance commands, for Windows, macOS and Linux.
//!
//! What is deliberately NOT here, because cleaning it does harm: Windows
//! Prefetch (Windows trims it and launches get slower), WinSxS by hand,
//! pagefile/hiberfil, macOS language files and app thinning (break code
//! signatures), the macOS sleep image, browser profile data other than its
//! caches (cookies, history, logins), and package-manager databases.

use crate::caches::{children, which, How, Risk, Target};
use crate::util::{self, home};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn t(id: &'static str, name: &'static str, eco: &'static str, risk: Risk, about: &'static str, paths: Vec<PathBuf>, how: How) -> Target {
    Target { id, name, eco, about, risk, paths, how, junk: true }
}

const DAY: u64 = 24;

fn admin(days: Option<u32>) -> How {
    How::Admin { days, pre: "", cmd: "", post: "" }
}

fn admin_cmd(cmd: &'static str) -> How {
    How::Admin { days: None, pre: "", cmd, post: "" }
}

/// Files in `dir` whose names match one of the extensions (no dot).
fn files_with(dir: &Path, exts: &[&str], prefix: Option<&[&str]>) -> Vec<PathBuf> {
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
fn chromium(user_data: &Path) -> Vec<PathBuf> {
    let mut v = children(user_data, |n| n == "Default" || n.starts_with("Profile ") || n == "Guest Profile", &["Cache", "Code Cache", "GPUCache", "Service Worker/CacheStorage"]);
    v.extend(["ShaderCache", "GrShaderCache", "GraphiteDawnCache", "component_crx_cache"].iter().map(|d| user_data.join(d)));
    v
}

/// Firefox keeps its disk cache apart from the profile.
fn firefox(profiles: &Path) -> Vec<PathBuf> {
    children(profiles, |_| true, &["cache2", "startupCache", "thumbnails"])
}

// ------------------------------------------------------------ drive walk

/// What a walk of the chosen drives found. Kept between a scan and the
/// clean that follows it, so cleaning doesn't walk every drive again.
#[derive(Default, Clone)]
struct DriveJunk {
    partials: Vec<PathBuf>,
    thumbs: Vec<PathBuf>,
    shader_caches: Vec<PathBuf>,
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
fn drive_junk(drives: Option<&[String]>, walk: bool) -> DriveJunk {
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

fn dedupe(mut v: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    v.retain(|p| seen.insert(p.to_string_lossy().to_ascii_lowercase()));
    v
}

pub fn targets(drives: Option<&[String]>, walk: bool) -> Vec<Target> {
    let h = home();
    let fake = util::fake_home().is_some();
    let dl = h.join("Downloads");
    let found = drive_junk(drives, walk);
    let mut partials = files_with(&dl, PARTIAL_EXT, None);
    partials.extend(found.partials);
    let mut v = vec![
        t("broken-downloads", "Unfinished downloads", "Downloads", Risk::Safe,
            "Downloads that never finished (.crdownload, .part, .download, .partial), in your Downloads folder and on every drive chosen in Settings. They can't be opened; download the file again if you still need it.",
            dedupe(partials), How::Delete),
        t("thumbs-db", "Thumbs.db files", "Downloads", Risk::Safe,
            "Hidden thumbnail caches older Windows versions and network shares leave in folders of pictures and videos, found on the chosen drives. Windows rebuilds them only if it needs them.",
            found.thumbs, How::Delete),
        t("steam-shaders", "Steam shader caches", "Apps", Risk::Slow,
            "Shaders Steam compiled for each game, in every Steam library on the chosen drives. Games rebuild them; the first launch after may stutter briefly.",
            found.shader_caches, How::Delete),
        t("old-installers", "Old disk images & installers", "Downloads", Risk::Data,
            "Disk images and installers in Downloads older than a week (.dmg, .iso, .pkg, .msi, .exe setup files). Usually installed already, but they are your files: check before deleting.",
            files_with(&dl, &["dmg", "iso", "pkg", "msi", "img"], None), How::OlderThan(7 * DAY)),
        t("recycle-bin", if cfg!(windows) { "Recycle Bin" } else { "Trash" }, "System", Risk::Data,
            "Files you deleted earlier, on every drive chosen in Settings. Emptying it can't be undone.",
            vec![], How::RecycleBin),
    ];

    if cfg!(windows) {
        let local = util::local_dir();
        let roaming = util::roaming_dir();
        let win = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()));
        let pd = PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into()));
        let explorer = local.join("Microsoft").join("Windows").join("Explorer");
        v.extend([
            t("wer-user", "Error reports (yours)", "System", Risk::Safe,
                "Crash and hang reports Windows Error Reporting keeps for your apps. Only reports older than 30 days are removed.",
                vec![local.join("Microsoft").join("Windows").join("WER")], How::OlderThan(30 * DAY)),
            t("thumbcache", "Thumbnail & icon cache", "System", Risk::Safe,
                "Explorer's thumbnail and icon databases. Rebuilt as you browse folders. Explorer keeps the current ones open, so those are skipped.",
                files_with(&explorer, &["db"], Some(&["thumbcache_", "iconcache_"])), How::Delete),
            t("inetcache", "Internet cache", "System", Risk::Safe,
                "Temporary Internet Files used by Windows, Office and older apps (Disk Cleanup's \"Temporary Internet Files\").",
                vec![local.join("Microsoft").join("Windows").join("INetCache")], How::Delete),
            t("chrome-cache", "Chrome cache", "Browsers", Risk::Safe,
                "Chrome's web, code and GPU caches for every profile. Cookies, history, passwords and tabs are not touched. Files in use while Chrome runs are skipped.",
                chromium(&local.join("Google").join("Chrome").join("User Data")), How::Delete),
            t("edge-cache", "Edge cache", "Browsers", Risk::Safe,
                "Microsoft Edge's web, code and GPU caches for every profile. Cookies, history and passwords are kept.",
                chromium(&local.join("Microsoft").join("Edge").join("User Data")), How::Delete),
            t("brave-cache", "Brave cache", "Browsers", Risk::Safe,
                "Brave's web, code and GPU caches for every profile. Cookies, history and wallets are kept.",
                chromium(&local.join("BraveSoftware").join("Brave-Browser").join("User Data")), How::Delete),
            t("firefox-cache", "Firefox cache", "Browsers", Risk::Safe,
                "Firefox's disk cache for every profile. Your profile (bookmarks, history, logins) is not touched.",
                firefox(&local.join("Mozilla").join("Firefox").join("Profiles")), How::Delete),
            t("chat-caches", "Discord, Slack & Teams caches", "Apps", Risk::Safe,
                "Web and GPU caches of Electron chat apps. Messages and logins are kept; quit the apps first for the most space.",
                ["discord", "Slack", "Microsoft\\Teams"]
                    .iter()
                    .flat_map(|a| ["Cache", "Code Cache", "GPUCache"].iter().map(|c| roaming.join(a).join(c)).collect::<Vec<_>>())
                    .collect(), How::Delete),
        ]);
        if !fake {
            v.extend([
                t("system-temp", "Windows temp", "System", Risk::Safe,
                    "C:\\Windows\\Temp, used by installers and services. Only items older than 7 days are removed; anything in use is skipped.",
                    vec![win.join("Temp")], admin(Some(7))),
                t("wu-cache", "Windows Update downloads", "System", Risk::Slow,
                    "Update packages Windows already downloaded. The update services are stopped while this runs and restarted after; anything still needed is downloaded again. Don't run it while an update is installing.",
                    vec![win.join("SoftwareDistribution").join("Download")],
                    How::Admin { days: None, pre: "Stop-Service wuauserv,bits -Force", cmd: "", post: "Start-Service wuauserv,bits" }),
                t("delivery-opt", "Delivery Optimization cache", "System", Risk::Safe,
                    "Update files Windows keeps to share with other PCs. Cleared with Microsoft's Delete-DeliveryOptimizationCache.",
                    vec![win.join("ServiceProfiles").join("NetworkService").join("AppData").join("Local").join("Microsoft").join("Windows").join("DeliveryOptimization").join("Cache")],
                    admin_cmd("Delete-DeliveryOptimizationCache -Force")),
                t("wer-system", "Error reports (system)", "System", Risk::Safe,
                    "Windows Error Reporting's archive and queue. ReportQueue can grow to many GB. Only reports older than 30 days are removed.",
                    ["ReportArchive", "ReportQueue", "Temp"].iter().map(|d| pd.join("Microsoft").join("Windows").join("WER").join(d)).collect(), admin(Some(30))),
                t("memory-dumps", "Memory dumps", "System", Risk::Slow,
                    "Dumps Windows writes after a blue screen. Useful only when diagnosing that crash; dumps older than 30 days are removed.",
                    vec![win.join("MEMORY.DMP"), win.join("Minidump")], admin(Some(30))),
                t("cbs-logs", "Windows servicing logs", "System", Risk::Safe,
                    "Old component-servicing (CBS) logs. The active log is kept; logs older than 30 days are removed.",
                    vec![win.join("Logs").join("CBS")], admin(Some(30))),
                t("component-store", "Component store cleanup", "System", Risk::Slow,
                    "Removes superseded Windows component versions with DISM. Safe and supported by Microsoft; takes several minutes. Size is known only after it runs.",
                    vec![], admin_cmd("Dism.exe /Online /Cleanup-Image /StartComponentCleanup")),
                t("windows-old", "Previous Windows installation", "System", Risk::Data,
                    "Windows.old from the last upgrade. Deleting it removes the option to roll back to your previous Windows version. Windows deletes it on its own about 10 days after an upgrade.",
                    [PathBuf::from(format!("{}\\", win.to_string_lossy().split('\\').next().unwrap_or("C:"))).join("Windows.old")].into_iter().filter(|p| p.exists()).collect(),
                    admin_cmd("cleanmgr.exe /autoclean")),
            ]);
        }
    } else if cfg!(target_os = "macos") {
        let lib = h.join("Library");
        let caches = lib.join("Caches");
        // Covered on the Disk cleanup page, or Apple-protected.
        const SKIP: &[&str] = &[
            "Homebrew", "pip", "pnpm", "Yarn", "CocoaPods", "deno", "go-build", "ms-playwright", "JetBrains", "Cypress",
            "electron", "electron-builder", "node-gyp", "pypoetry", "composer", "Google", "Firefox", "BraveSoftware",
            "Microsoft Edge", "Arc", "Mozilla.sccache", "ccache", "zig", "torch_extensions", "llama.cpp", "com.apple.Safari",
        ];
        let user_caches: Vec<PathBuf> = std::fs::read_dir(&caches)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .filter(|e| {
                        let n = e.file_name().to_string_lossy().to_string();
                        !n.starts_with("com.apple.") && !SKIP.contains(&n.as_str())
                    })
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        v.extend([
            t("user-caches", "App caches", "System", Risk::Safe,
                "~/Library/Caches for your apps. Apps rebuild them as needed. Apple's own caches and the developer caches on the Disk cleanup page are left out.",
                user_caches, How::Delete),
            t("user-logs", "Your logs", "System", Risk::Safe,
                "~/Library/Logs: app logs and crash reports. Only items older than 7 days are removed.",
                vec![lib.join("Logs")], How::OlderThan(7 * DAY)),
            t("chrome-cache", "Chrome cache", "Browsers", Risk::Safe,
                "Chrome's disk cache for every profile. Cookies, history and passwords are kept.",
                vec![caches.join("Google").join("Chrome")], How::Delete),
            t("brave-cache", "Brave cache", "Browsers", Risk::Safe,
                "Brave's disk cache. Cookies, history and wallets are kept.",
                vec![caches.join("BraveSoftware").join("Brave-Browser")], How::Delete),
            t("edge-cache", "Edge cache", "Browsers", Risk::Safe,
                "Microsoft Edge's disk cache. Cookies, history and passwords are kept.",
                vec![caches.join("Microsoft Edge")], How::Delete),
            t("firefox-cache", "Firefox cache", "Browsers", Risk::Safe,
                "Firefox's disk cache for every profile. Your profile is not touched.",
                firefox(&caches.join("Firefox").join("Profiles")), How::Delete),
            t("safari-cache", "Safari cache", "Browsers", Risk::Safe,
                "Safari's web cache. Needs Full Disk Access for Dev Cleaner (System Settings → Privacy & Security); without it, nothing is removed.",
                vec![caches.join("com.apple.Safari")], How::Delete),
            t("ios-device-support", "iOS DeviceSupport", "Apple", Risk::Slow,
                "Debug symbols Xcode copies from every iPhone/iPad OS version you connected. Copied again the next time a device connects.",
                vec![lib.join("Developer").join("Xcode").join("iOS DeviceSupport")], How::Delete),
            t("sim-unavailable", "Unavailable simulators", "Apple", Risk::Safe,
                "Simulators for iOS versions no longer installed. Removed with xcrun simctl delete unavailable.",
                vec![], How::Command { tool: "xcrun", args: &["simctl", "delete", "unavailable"] }),
            t("xcode-archives", "Xcode archives", "Apple", Risk::Data,
                "Archives of apps you built for release. Keep the ones you shipped if you may need their symbols.",
                vec![lib.join("Developer").join("Xcode").join("Archives")], How::Delete),
            t("ios-backups", "iPhone & iPad backups", "Apple", Risk::Data,
                "Local device backups. Apple: a deleted backup can't be recovered. Needs Full Disk Access.",
                vec![lib.join("Application Support").join("MobileSync").join("Backup")], How::Delete),
            t("mail-downloads", "Mail attachments cache", "Apps", Risk::Slow,
                "Attachments Mail saved when you opened them. They stay in the messages and download again when opened. Needs Full Disk Access.",
                vec![lib.join("Containers").join("com.apple.mail").join("Data").join("Library").join("Mail Downloads")], How::Delete),
            t("quicklook", "Quick Look cache", "System", Risk::Safe,
                "Preview thumbnails for Quick Look. Reset with qlmanage -r cache.",
                vec![], How::Command { tool: "qlmanage", args: &["-r", "cache"] }),
        ]);
        if !fake {
            v.extend([
                t("system-logs", "System logs", "System", Risk::Safe,
                    "/Library/Logs and /var/log. Only log files older than 7 days are removed; the active logs are kept.",
                    vec![PathBuf::from("/Library/Logs"), PathBuf::from("/private/var/log")], admin(Some(7))),
                t("tm-snapshots", "Time Machine local snapshots", "System", Risk::Slow,
                    "Local snapshots macOS keeps between backups. Thinned with tmutil; your backups on the backup disk are not touched. Size is known only after it runs.",
                    vec![], admin_cmd("/usr/bin/tmutil thinlocalsnapshots / 999999999999 4")),
            ]);
        }
    } else {
        let cache = util::cache_dir();
        v.extend([
            t("thumbnails", "Thumbnail cache", "System", Risk::Safe,
                "Thumbnails file managers created. Rebuilt as you browse.",
                vec![cache.join("thumbnails")], How::Delete),
            t("chrome-cache", "Chrome / Chromium cache", "Browsers", Risk::Safe,
                "Disk caches of Chrome and Chromium. Cookies, history and passwords live in ~/.config and are kept.",
                vec![cache.join("google-chrome"), cache.join("chromium")], How::Delete),
            t("brave-cache", "Brave cache", "Browsers", Risk::Safe,
                "Brave's disk cache. Your profile in ~/.config is kept.",
                vec![cache.join("BraveSoftware")], How::Delete),
            t("edge-cache", "Edge cache", "Browsers", Risk::Safe,
                "Microsoft Edge's disk cache.",
                vec![cache.join("microsoft-edge")], How::Delete),
            t("firefox-cache", "Firefox cache", "Browsers", Risk::Safe,
                "Firefox's disk cache for every profile. Your profile is not touched.",
                firefox(&cache.join("mozilla").join("firefox")), How::Delete),
            t("mesa-shaders", "Mesa shader cache", "System", Risk::Safe,
                "Shaders compiled by the open-source GPU drivers. Rebuilt as apps and games run.",
                vec![cache.join("mesa_shader_cache"), cache.join("mesa_shader_cache_db"), cache.join("nvidia").join("GLCache")], How::Delete),
            t("flatpak-unused", "Unused Flatpak runtimes", "System", Risk::Safe,
                "Runtimes and extensions no installed app uses any more.",
                vec![], How::Command { tool: "flatpak", args: &["uninstall", "--unused", "-y"] }),
        ]);
        if !fake {
            v.push(t("journal", "System journal", "System", Risk::Safe,
                "Archived systemd journal files older than 7 days. The active journal is kept.",
                vec![PathBuf::from("/var/log/journal")], admin_cmd("journalctl --vacuum-time=7d")));
            v.push(t("coredumps", "Core dumps", "System", Risk::Safe,
                "Memory dumps of crashed programs kept by systemd-coredump. Only useful when debugging that crash.",
                vec![PathBuf::from("/var/lib/systemd/coredump")], admin(Some(0))));
            for (tool, id, name, path, cmd) in [
                ("apt-get", "apt-cache", "APT package cache", "/var/cache/apt/archives", "apt-get clean"),
                ("dnf", "dnf-cache", "DNF package cache", "/var/cache/dnf", "dnf clean all"),
                ("paccache", "pacman-cache", "Pacman package cache", "/var/cache/pacman/pkg", "paccache -rk2"),
            ] {
                if which(tool).is_some() {
                    v.push(t(id, name, "System", Risk::Safe,
                        "Downloaded package files the package manager keeps after installing. Installed software is not affected.",
                        vec![PathBuf::from(path)], admin_cmd(cmd)));
                }
            }
            if which("snap").is_some() {
                v.push(t("snap-old", "Old Snap revisions", "System", Risk::Safe,
                    "Disabled older revisions Snap keeps for rollback. The current revision of every snap stays.",
                    vec![], admin_cmd("snap list --all | awk '/disabled/{print $1, $3}' | while read n r; do snap remove \"$n\" --revision=\"$r\"; done")));
            }
        }
    }
    v
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

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn ps_quote(s: &str) -> String {
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
    let script = admin_script(items);
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
        let t1 = Target {
            id: "x", name: "x", eco: "x", about: "x", risk: Risk::Safe, junk: true,
            paths: vec![PathBuf::from(if cfg!(windows) { r"C:\Win dows\Te'mp" } else { "/var/lo'g" })],
            how: admin(Some(7)),
        };
        let t2 = Target { id: "y", name: "y", eco: "y", about: "y", risk: Risk::Safe, junk: true, paths: vec![], how: How::Admin { days: None, pre: "STOP", cmd: "RUN", post: "START" } };
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
