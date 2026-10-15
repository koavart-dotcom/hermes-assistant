//! Tauri commands callable from the UI. Every exported type is mirrored to
//! `src/types/generated/` by ts-rs (regenerated on `cargo test`).

use hermes_core::events::SetupEvent;
use hermes_core::gateway::Gateway;
use hermes_core::ollama::Ollama;
use hermes_core::setup::{SetupOptions, SetupPlan};
use hermes_core::status::StatusSnapshot;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::state::{AppState, RunningSetup};

pub const SETUP_EVENT: &str = "setup:event";

/// Lifecycle state of the bundled Hermes Agent gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum HermesState {
    NotInstalled,
    Stopped,
    Running,
}

/// Static information about the shell and the agent it supervises.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub hermes_state: HermesState,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn app_info(app: AppHandle, state: State<'_, AppState>) -> Result<AppInfo, String> {
    let pkg = app.package_info();
    let layout = &state.ctx.layout;
    let hermes_state = if !layout.bootstrap_marker().exists() {
        HermesState::NotInstalled
    } else if Gateway::new(layout.clone(), None).is_healthy().await {
        HermesState::Running
    } else {
        HermesState::Stopped
    };
    Ok(AppInfo {
        name: pkg.name.clone(),
        version: pkg.version.to_string(),
        hermes_state,
    })
}

#[tauri::command]
pub fn setup_plan(state: State<'_, AppState>) -> SetupPlan {
    state.ctx.plan()
}

#[tauri::command]
pub fn setup_start(
    app: AppHandle,
    state: State<'_, AppState>,
    options: SetupOptions,
) -> Result<(), String> {
    let mut slot = state.setup.lock().map_err(err)?;
    if slot.is_some() {
        return Err("setup is already running".into());
    }
    let cancel = CancellationToken::new();
    *slot = Some(RunningSetup {
        cancel: cancel.clone(),
    });
    drop(slot);

    let ctx = state.ctx.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SetupEvent>();

    // Forward core events to the webview.
    let forward_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let _ = forward_app.emit(SETUP_EVENT, &ev);
        }
    });

    tauri::async_runtime::spawn(async move {
        let result = hermes_core::setup::run(&ctx, &options, &tx, &cancel).await;
        let state = app.state::<AppState>();
        if let Ok(Some(child)) = result {
            if let Ok(mut c) = state.ollama_child.lock() {
                *c = Some(child);
            }
        }
        if let Ok(mut slot) = state.setup.lock() {
            *slot = None;
        }
    });
    Ok(())
}

#[tauri::command]
pub fn setup_cancel(state: State<'_, AppState>) -> Result<(), String> {
    let slot = state.setup.lock().map_err(err)?;
    if let Some(run) = slot.as_ref() {
        run.cancel.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn status_snapshot(state: State<'_, AppState>) -> Result<StatusSnapshot, String> {
    let layout = state.ctx.layout.clone();
    let ollama = Ollama::new(layout.clone());
    let api_key = hermes_core::secrets::api_server_key(&layout).ok();
    let gateway = Gateway::new(layout.clone(), api_key);
    hermes_core::status::snapshot(&layout, &state.ctx.pin.version, &ollama, &gateway)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn gateway_start(state: State<'_, AppState>) -> Result<(), String> {
    let layout = state.ctx.layout.clone();
    let api_key = hermes_core::secrets::api_server_key(&layout).ok();
    let ollama = Ollama::new(layout.clone());
    if ollama.is_installed() {
        if let Ok(Some(child)) = ollama.start().await {
            if let Ok(mut c) = state.ollama_child.lock() {
                *c = Some(child);
            }
        }
    }
    Gateway::new(layout, api_key)
        .start(None, &CancellationToken::new())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn gateway_stop(state: State<'_, AppState>) -> Result<(), String> {
    let layout = state.ctx.layout.clone();
    Gateway::new(layout, None)
        .stop(None, &CancellationToken::new())
        .await
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermes_state_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&HermesState::NotInstalled).unwrap(),
            "\"not_installed\""
        );
    }
}
