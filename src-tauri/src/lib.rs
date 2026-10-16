//! Hermes shell: Tauri application that supervises Hermes Agent and Ollama,
//! exposes typed commands to the React UI and a loopback control API for
//! remote administration (see `hermes_core::control`).

pub mod commands;
pub mod state;
pub mod update;

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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

            // Forward chat events (tagged with their session) to the webview.
            let chat_handle = app.handle().clone();
            let mut chat_rx = sup.subscribe_chat();
            tauri::async_runtime::spawn(async move {
                loop {
                    match chat_rx.recv().await {
                        Ok(ev) => {
                            let envelope = hermes_core::chat::ChatEnvelope {
                                session_id: ev.session_id,
                                event: ev.event,
                            };
                            let _ = chat_handle.emit(commands::CHAT_EVENT, &envelope);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("chat event forwarder lagged by {n} events");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            // Start at Windows login (per-user Run key), so Ollama and the
            // control API come back after a reboot without a manual launch.
            if let Ok(exe) = std::env::current_exe() {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = hermes_core::autostart::enable(&exe).await {
                        log::warn!("autostart registration failed: {e}");
                    }
                });
            }

            // Restart the Ollama sidecar with the current environment.
            let sup_ollama = sup.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = sup_ollama.ollama_refresh().await {
                    log::warn!("ollama refresh failed: {e}");
                }
            });

            // Bring the Hermes web server (voice endpoints) up when voice was set up.
            let sup_voice = sup.clone();
            tauri::async_runtime::spawn(async move {
                match sup_voice.voice_ensure_server().await {
                    Ok(true) => log::info!("voice server ready"),
                    Ok(false) => {}
                    Err(e) => log::warn!("voice server start failed: {e}"),
                }
            });

            // Keep the bundled permission plugin installed and enabled.
            let sup_plugins = sup.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = sup_plugins.plugins_ensure().await {
                    log::warn!("plugin refresh failed: {e}");
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
            commands::chat_sessions,
            commands::chat_create_session,
            commands::chat_delete_session,
            commands::chat_messages,
            commands::chat_send,
            commands::chat_stop,
            commands::chat_approve,
            update::update_check,
            update::update_install,
            commands::permissions_log,
            commands::permissions_policy_get,
            commands::permissions_policy_set,
            commands::voice_status,
            commands::voice_setup,
            commands::voice_transcribe,
            commands::voice_speak,
            commands::quick_commands,
            commands::quick_match,
            commands::quick_run,
            commands::quick_import_voices,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
