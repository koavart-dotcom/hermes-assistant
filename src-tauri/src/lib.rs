//! Hermes shell: Tauri application that supervises Hermes Agent and Ollama,
//! exposes typed commands to the React UI and a loopback control API for
//! remote administration (see `hermes_core::control`).

pub mod commands;
pub mod state;

use hermes_core::control::{self, ControlConfig};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            let state = state::AppState::init(app.handle())?;
            let sup = state.supervisor.clone();
            app.manage(state);

            // Forward supervisor events to the webview.
            let handle = app.handle().clone();
            let mut rx = sup.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            let _ = handle.emit(commands::SETUP_EVENT, &ev.event);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("webview event forwarder lagged by {n} events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            // Control API on loopback (reached remotely through an SSH tunnel).
            match control::token(&sup.ctx.layout) {
                Ok(token) => {
                    let pkg = app.package_info();
                    let cfg = ControlConfig {
                        port: control::port_from_env(),
                        token,
                        app_name: pkg.name.clone(),
                        app_version: pkg.version.to_string(),
                        log_dir: app.path().app_log_dir().unwrap_or_default(),
                    };
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = control::serve(sup, cfg).await {
                            log::error!("control api stopped: {e}");
                        }
                    });
                }
                Err(e) => log::error!("control api disabled, token unavailable: {e}"),
            }
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
