//! Remembering where the window was: position, size, maximized, and so which
//! monitor it sat on.
//!
//! Hand-rolled rather than `tauri-plugin-window-state`, for two Windows
//! problems seen with undecorated windows:
//!
//! 1. An undecorated window keeps the style bits of a decorated one, so the
//!    inner size Tauri reports disagrees with the real client rect.
//! 2. Converting through logical pixels on a fractional scale factor rounds the
//!    same way every launch, so the window creeps smaller on each restart.
//!
//! So everything is physical pixels, the client size is read from the OS, and
//! a correction pass after the window is shown fixes what the frame
//! arithmetic got wrong. A saved position on a monitor that is no longer
//! attached is dropped, so the window never opens out of reach.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{Manager, PhysicalPosition, PhysicalSize, Runtime};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// Where the app keeps its own small files: the per-user config folder, or
/// the `data` folder beside the exe in portable mode.
pub fn data_dir<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    let dir = match crate::util::portable_dir() {
        Some(d) => d,
        None => app.path().app_config_dir().ok()?,
    };
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}

fn file<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<PathBuf> {
    Some(data_dir(app)?.join("window-state.json"))
}

pub fn load<R: Runtime>(app: &tauri::AppHandle<R>) -> Option<WindowState> {
    let raw = std::fs::read_to_string(file(app)?).ok()?;
    let state: WindowState = serde_json::from_str(&raw).ok()?;
    // A size too small to use counts as no state, so one bad write can never
    // leave the window unopenable.
    (state.width >= 200 && state.height >= 150).then_some(state)
}

pub fn store<R: Runtime>(app: &tauri::AppHandle<R>, state: &WindowState) {
    let (Some(path), Ok(json)) = (file(app), serde_json::to_string(state)) else {
        return;
    };
    // Write then rename, so a crash mid-write never leaves half a file.
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// The window's real client size, straight from the OS.
#[cfg(windows)]
fn client_size<R: Runtime>(window: &tauri::Window<R>) -> Option<(u32, u32)> {
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    let hwnd = window.hwnd().ok()?.0 as HWND;
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: hwnd is a live window owned by Tauri; GetClientRect only writes
    // into the rect we own.
    let ok = unsafe { GetClientRect(hwnd, &mut r) };
    (ok != 0).then(|| ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32))
}

#[cfg(not(windows))]
fn client_size<R: Runtime>(window: &tauri::Window<R>) -> Option<(u32, u32)> {
    let s = window.inner_size().ok()?;
    Some((s.width, s.height))
}

/// Current geometry, or None when there is nothing worth saving. Minimized
/// windows report an off-screen position, so they save nothing. A maximized
/// window keeps the last normal geometry and only sets the flag, so
/// un-maximizing next launch returns to where it was.
pub fn capture<R: Runtime>(window: &tauri::Window<R>, previous: Option<WindowState>) -> Option<WindowState> {
    if window.is_minimized().unwrap_or(false) {
        return None;
    }
    if window.is_maximized().unwrap_or(false) {
        return Some(match previous {
            Some(p) => WindowState { maximized: true, ..p },
            None => {
                let pos = window.outer_position().ok()?;
                let (width, height) = client_size(window)?;
                WindowState { x: pos.x, y: pos.y, width, height, maximized: true }
            }
        });
    }
    let pos = window.outer_position().ok()?;
    let (width, height) = client_size(window)?;
    (width >= 200 && height >= 150).then_some(WindowState { x: pos.x, y: pos.y, width, height, maximized: false })
}

/// Put the window back before it is shown.
pub fn restore<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(state) = load(app) else { return };
    let Some(window) = app.get_webview_window("main") else { return };
    if is_off_screen(&window, &state) {
        let _ = window.center();
        return;
    }
    let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
    let _ = window.set_size(PhysicalSize::new(state.width, state.height));
}

/// Ask for a size, measure what arrived, ask again for the difference. No
/// frame thickness is assumed anywhere.
pub fn settle<R: Runtime>(window: &tauri::Window<R>, want: &WindowState) {
    if window.is_maximized().unwrap_or(false) {
        return;
    }
    let mut request = (want.width as i64, want.height as i64);
    for _ in 0..3 {
        let Some((w, h)) = client_size(window) else { return };
        let dw = w as i64 - want.width as i64;
        let dh = h as i64 - want.height as i64;
        if dw == 0 && dh == 0 {
            return;
        }
        request = ((request.0 - dw).clamp(200, 20_000), (request.1 - dh).clamp(200, 20_000));
        let _ = window.set_size(PhysicalSize::new(request.0 as u32, request.1 as u32));
    }
}

/// True when the saved title-bar strip would not be on any attached monitor,
/// e.g. the external display it was on has been unplugged.
fn is_off_screen<R: Runtime>(window: &tauri::WebviewWindow<R>, state: &WindowState) -> bool {
    let Ok(monitors) = window.available_monitors() else { return false };
    if monitors.is_empty() {
        return false;
    }
    let rects: Vec<(i32, i32, u32, u32)> = monitors
        .iter()
        .map(|m| (m.position().x, m.position().y, m.size().width, m.size().height))
        .collect();
    strip_off_screen(state, &rects)
}

/// The pure part of the test, so it can be unit tested.
fn strip_off_screen(state: &WindowState, monitors: &[(i32, i32, u32, u32)]) -> bool {
    const MARGIN: i32 = 48;
    monitors.iter().all(|&(mx, my, mw, mh)| {
        let right = mx + mw as i32;
        let bottom = my + mh as i32;
        state.x > right - MARGIN
            || state.x + (state.width as i32) < mx + MARGIN
            || state.y > bottom - 24
            || state.y + MARGIN < my
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: WindowState = WindowState { x: 2100, y: 120, width: 1200, height: 780, maximized: false };

    #[test]
    fn window_on_second_monitor_is_kept_while_it_is_attached() {
        let two = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
        assert!(!strip_off_screen(&S, &two));
    }

    #[test]
    fn window_on_unplugged_monitor_is_dropped() {
        let one = [(0, 0, 1920, 1080)];
        assert!(strip_off_screen(&S, &one));
    }

    #[test]
    fn monitor_left_of_primary_with_negative_coordinates() {
        let s = WindowState { x: -1800, y: 40, ..S };
        assert!(!strip_off_screen(&s, &[(-1920, 0, 1920, 1080), (0, 0, 1920, 1080)]));
        assert!(strip_off_screen(&s, &[(0, 0, 1920, 1080)]));
    }
}
