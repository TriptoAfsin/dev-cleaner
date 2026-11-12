//! Memory pressure sampler and history. (stub)

/// Start the background sampler. Called once from setup.
pub fn start() {}

#[tauri::command]
pub fn pressure_now() {}
#[tauri::command]
pub fn pressure_history() {}
#[tauri::command]
pub fn memory_growth() {}
