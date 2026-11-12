//! Orphaned dev processes, the WSL VM and Docker containers. (stub)

#[tauri::command]
pub fn list_orphans() {}
#[tauri::command]
pub fn wsl_status() {}
#[tauri::command]
pub fn wsl_reclaim() {}
#[tauri::command]
pub fn wsl_shutdown() {}
#[tauri::command]
pub fn list_containers() {}
#[tauri::command]
pub fn stop_containers() {}
