//! What extra rights the app has, and how to get more, per OS:
//! - Windows: restart elevated through UAC.
//! - macOS: no root relaunch (running a GUI as root is unsupported); what
//!   matters is Full Disk Access for protected folders, and admin cleanups
//!   ask for a password through osascript.
//! - Linux: admin cleanups go through pkexec (polkit), one prompt per clean.

use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Privileges {
    pub os: &'static str,
    /// Running elevated (Windows admin, or root elsewhere).
    pub is_admin: bool,
    /// Whether "Restart as administrator" is offered (Windows only).
    pub can_relaunch: bool,
    /// macOS Full Disk Access: Some(true/false), or None when it can't be told.
    pub full_disk_access: Option<bool>,
    /// Linux: pkexec is available for admin cleanups.
    pub pkexec: bool,
    pub in_flatpak: bool,
}

pub fn get() -> Privileges {
    Privileges {
        os: std::env::consts::OS,
        is_admin: crate::sysinfo_dev::is_admin(),
        can_relaunch: cfg!(windows),
        full_disk_access: full_disk_access(),
        pkexec: cfg!(target_os = "linux") && crate::util::which("pkexec").is_some(),
        in_flatpak: crate::util::in_flatpak(),
    }
}

/// There is no API for Full Disk Access; read a folder only it unlocks.
/// Permission denied means no; a folder that doesn't exist says nothing.
fn full_disk_access() -> Option<bool> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let home = crate::util::home();
    for p in [
        home.join("Library/Safari"),
        home.join("Library/Containers/com.apple.mail"),
        home.join("Library/Application Support/com.apple.TCC"),
        Path::new("/Library/Preferences/com.apple.TimeMachine.plist").to_path_buf(),
    ] {
        let r = if p.is_file() { std::fs::File::open(&p).map(|_| ()) } else { std::fs::read_dir(&p).map(|_| ()) };
        match r {
            Ok(()) => return Some(true),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Some(false),
            Err(_) => continue,
        }
    }
    None
}

/// macOS: open System Settings at Privacy & Security → Full Disk Access.
pub fn open_full_disk_access() -> bool {
    cfg!(target_os = "macos")
        && crate::util::run_ok(
            Path::new("/usr/bin/open"),
            &["x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"],
            std::time::Duration::from_secs(10),
        )
}

/// Windows: start an elevated copy of this exe through UAC. It is told to
/// wait for this process to exit first, so the two never share the webview
/// profile at once. False if the UAC prompt was cancelled.
#[cfg(windows)]
pub fn relaunch_elevated() -> bool {
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let Ok(exe) = std::env::current_exe() else { return false };
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide(&format!("--wait-pid={}", std::process::id()));
    // SAFETY: the struct is zeroed, sized, and every string outlives the call.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        ShellExecuteExW(&mut info) != 0
    }
}

#[cfg(not(windows))]
pub fn relaunch_elevated() -> bool {
    false
}

/// Called first thing on startup: an elevated relaunch waits (up to 15 s)
/// for the copy that started it to close.
pub fn wait_for_parent() {
    let Some(pid) = std::env::args().find_map(|a| a.strip_prefix("--wait-pid=").and_then(|p| p.parse::<u32>().ok())) else {
        return;
    };
    let target = sysinfo::Pid::from_u32(pid);
    let mut sys = sysinfo::System::new();
    for _ in 0..150 {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[target]), true);
        if sys.process(target).is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
