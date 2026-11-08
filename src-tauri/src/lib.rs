mod caches;
mod holders;
mod junk;
mod langs;
mod privileges;
mod configs;
mod models;
mod monitor;
mod ports;
mod projects;
mod sysinfo_dev;
mod util;
mod winstate;

use monitor::Monitor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State, WindowEvent};

type R<T> = Result<T, String>;

/// Run blocking work off the main thread so the window never stalls.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> R<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn live_stats(m: State<Arc<Monitor>>) -> monitor::LiveStats {
    monitor::live(&m)
}

#[tauri::command]
async fn list_processes(m: State<'_, Arc<Monitor>>) -> R<Vec<monitor::ProcGroup>> {
    let m = Arc::clone(&m);
    blocking(move || monitor::processes(&m)).await
}

#[tauri::command]
async fn kill_processes(m: State<'_, Arc<Monitor>>, pids: Vec<u32>) -> R<monitor::KillResult> {
    let m = Arc::clone(&m);
    blocking(move || monitor::kill(&m, &pids)).await
}

#[tauri::command]
async fn free_memory(m: State<'_, Arc<Monitor>>) -> R<monitor::FreeResult> {
    let m = Arc::clone(&m);
    blocking(move || monitor::free_memory(&m)).await
}

#[tauri::command]
async fn scan_caches(app: AppHandle, drives: Option<Vec<String>>) -> R<Vec<caches::CacheItem>> {
    blocking(move || caches::scan(&app, drives.as_deref())).await
}

#[tauri::command]
async fn clean_caches(app: AppHandle, ids: Vec<String>) -> R<Vec<caches::CleanEvent>> {
    blocking(move || caches::clean(&app, &ids)).await
}

#[tauri::command]
async fn list_ports(all: bool, extra: Vec<u16>) -> R<Vec<ports::PortEntry>> {
    blocking(move || ports::list(all, &extra)).await
}

#[tauri::command]
async fn system_info() -> R<sysinfo_dev::SystemInfo> {
    blocking(sysinfo_dev::collect).await
}

#[tauri::command]
async fn scan_projects(app: AppHandle, roots: Vec<String>, depth: Option<u32>) -> R<Vec<projects::Artifact>> {
    blocking(move || projects::scan(&app, &roots, depth.unwrap_or(7))).await
}

/// Mounted drives, for the Drives setting.
#[tauri::command]
async fn list_drives() -> R<Vec<sysinfo_dev::Drive>> {
    blocking(sysinfo_dev::drives).await
}

#[tauri::command]
fn cancel_project_scan() {
    projects::CANCEL.store(true, Ordering::Relaxed);
}

#[tauri::command]
async fn delete_artifacts(app: AppHandle, paths: Vec<String>) -> R<Vec<projects::DeleteEvent>> {
    blocking(move || projects::delete(&app, &paths)).await
}

#[tauri::command]
async fn scan_models(app: AppHandle, extra_roots: Vec<String>) -> R<Vec<models::Model>> {
    blocking(move || models::scan(&app, &extra_roots)).await
}

#[tauri::command]
fn cancel_model_scan() {
    models::CANCEL.store(true, Ordering::Relaxed);
}

#[tauri::command]
async fn delete_models(app: AppHandle, ids: Vec<String>) -> R<Vec<models::DeleteEvent>> {
    blocking(move || models::delete(&app, &ids)).await
}

#[tauri::command]
async fn list_config_sources() -> R<Vec<configs::ConfigTool>> {
    blocking(configs::list_sources).await
}

#[tauri::command]
async fn export_configs(app: AppHandle, item_ids: Vec<String>, dest_path: String) -> R<configs::ExportResult> {
    blocking(move || configs::export(&app, &item_ids, &dest_path)).await?
}

#[tauri::command]
async fn read_bundle(zip_path: String) -> R<configs::BundleInfo> {
    blocking(move || configs::read_bundle(&zip_path)).await?
}

#[tauri::command]
async fn import_configs(app: AppHandle, zip_path: String, item_ids: Vec<String>) -> R<configs::ImportResult> {
    blocking(move || configs::import(&app, &zip_path, &item_ids)).await?
}

#[tauri::command]
async fn list_config_backups() -> R<Vec<configs::BackupInfo>> {
    blocking(configs::list_backups).await
}

#[tauri::command]
async fn undo_config_import(backup_id: String) -> R<configs::UndoResult> {
    blocking(move || configs::undo(&backup_id)).await?
}

#[tauri::command]
async fn list_migrations() -> R<Vec<configs::Migration>> {
    blocking(configs::list_migrations).await
}

#[tauri::command]
async fn plan_migration(id: String, options: configs::MigrateOptions) -> R<Vec<configs::PlanStep>> {
    blocking(move || configs::plan_migration(&id, &options)).await?
}

#[tauri::command]
async fn run_migration(app: AppHandle, id: String, options: configs::MigrateOptions) -> R<configs::MigrationResult> {
    blocking(move || configs::run_migration(&app, &id, &options)).await?
}

#[tauri::command]
fn is_admin() -> bool {
    sysinfo_dev::is_admin()
}

#[tauri::command]
fn relaunch_as_admin(app: AppHandle) -> bool {
    // Save where the window is first, so the elevated copy opens in place.
    if let Some(w) = app.get_webview_window("main") {
        save_now(&w.as_ref().window());
    }
    let ok = privileges::relaunch_elevated();
    if ok {
        app.exit(0);
    }
    ok
}

#[tauri::command]
async fn privileges_info() -> R<privileges::Privileges> {
    blocking(privileges::get).await
}

#[tauri::command]
fn open_full_disk_access() -> bool {
    privileges::open_full_disk_access()
}

/// Write a PNG (or any file) the user picked a path for in the save dialog.
#[tauri::command]
fn save_file(path: String, base64: String) -> R<()> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(base64.trim()).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())
}

/// Settings live in a file, not only in the webview's localStorage: WebView2
/// flushes localStorage lazily, and a quick quit could lose a change.
#[tauri::command]
fn load_settings(app: AppHandle) -> Option<serde_json::Value> {
    let path = winstate::data_dir(&app)?.join("settings.json");
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: serde_json::Value) -> R<()> {
    let dir = winstate::data_dir(&app).ok_or("no data folder")?;
    let tmp = dir.join("settings.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join("settings.json")).map_err(|e| e.to_string())
}

#[tauri::command]
fn platform() -> &'static str {
    std::env::consts::OS
}

/// Shown from the frontend once React has painted, so the window never
/// appears empty or at the wrong size first.
#[tauri::command]
fn reveal_window(window: tauri::Window) -> R<bool> {
    let maximized = winstate::load(window.app_handle()).is_some_and(|w| w.maximized);
    if maximized {
        window.maximize().map_err(|e| e.to_string())?;
    }
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_focus();
    Ok(maximized)
}

#[tauri::command]
fn settle_window(window: tauri::Window) {
    if let Some(want) = winstate::load(window.app_handle()) {
        winstate::settle(&window, &want);
    }
}

fn save_now<Rt: tauri::Runtime>(window: &tauri::Window<Rt>) {
    let app = window.app_handle();
    if let Some(state) = winstate::capture(window, winstate::load(app)) {
        winstate::store(app, &state);
    }
}

const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    privileges::wait_for_parent();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(Monitor::new()))
        .setup(|app| {
            // Built here rather than from the config file so portable mode can
            // keep the webview's storage beside the exe. Still created hidden.
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .expect("main window in tauri.conf.json");
            let mut window = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?;
            if let Some(dir) = util::portable_dir() {
                window = window.data_directory(dir.join("webview"));
            }
            window.build()?;
            // Put it back where it was before anyone can see it.
            winstate::restore(app.handle());
            Ok(())
        })
        // Save during moves and resizes too, so a crash or kill keeps the last
        // position. A drag fires hundreds of events: one thread waits for the
        // quiet moment and writes once.
        .on_window_event({
            let due: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
            let waiting = Arc::new(AtomicBool::new(false));
            move |window, event| {
                match event {
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => {}
                    WindowEvent::CloseRequested { .. } => {
                        save_now(window);
                        return;
                    }
                    _ => return,
                }
                if let Ok(mut slot) = due.lock() {
                    *slot = Some(Instant::now() + SAVE_DEBOUNCE);
                }
                if waiting.swap(true, Ordering::SeqCst) {
                    return;
                }
                let window = window.clone();
                let due = Arc::clone(&due);
                let waiting = Arc::clone(&waiting);
                std::thread::spawn(move || {
                    while let Some(at) = due.lock().ok().and_then(|s| *s) {
                        let remaining = at.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        std::thread::sleep(remaining);
                    }
                    if let Ok(mut slot) = due.lock() {
                        *slot = None;
                    }
                    waiting.store(false, Ordering::SeqCst);
                    save_now(&window);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            live_stats,
            list_processes,
            kill_processes,
            free_memory,
            scan_caches,
            clean_caches,
            list_ports,
            system_info,
            scan_projects,
            cancel_project_scan,
            delete_artifacts,
            scan_models,
            cancel_model_scan,
            delete_models,
            list_config_sources,
            export_configs,
            read_bundle,
            import_configs,
            list_config_backups,
            undo_config_import,
            list_migrations,
            plan_migration,
            run_migration,
            is_admin,
            relaunch_as_admin,
            privileges_info,
            open_full_disk_access,
            platform,
            list_drives,
            load_settings,
            save_settings,
            save_file,
            reveal_window,
            settle_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Dev Cleaner");
}
