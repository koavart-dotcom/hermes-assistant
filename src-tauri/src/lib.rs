//! Hermes shell: Tauri application that supervises Hermes Agent and Ollama
//! and exposes typed commands to the React UI.

pub mod commands;
pub mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            let state = state::AppState::init(app.handle())?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::setup_plan,
            commands::setup_start,
            commands::setup_cancel,
            commands::status_snapshot,
            commands::gateway_start,
            commands::gateway_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
