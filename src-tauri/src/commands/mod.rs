//! Tauri commands callable from the UI. Every exported type is mirrored to
//! `src/types/generated/` by ts-rs (regenerated on `cargo test`). All real
//! work lives in `hermes_core::supervisor`, which the control API shares.

use hermes_core::chat::{ChatMessage, SessionSummary};
use hermes_core::gateway::Gateway;
use hermes_core::setup::{SetupOptions, SetupPlan};
use hermes_core::status::StatusSnapshot;
use serde::Serialize;
use tauri::{AppHandle, State};
use ts_rs::TS;

use crate::state::AppState;

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
    let layout = &state.supervisor.ctx.layout;
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
    state.supervisor.plan()
}

#[tauri::command]
pub fn setup_start(state: State<'_, AppState>, options: SetupOptions) -> Result<(), String> {
    let fut = state.supervisor.start_setup(options).map_err(err)?;
    tauri::async_runtime::spawn(fut);
    Ok(())
}

#[tauri::command]
pub fn setup_cancel(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.cancel_setup();
    Ok(())
}

#[tauri::command]
pub async fn status_snapshot(state: State<'_, AppState>) -> Result<StatusSnapshot, String> {
    state.supervisor.status().await.map_err(err)
}

#[tauri::command]
pub async fn gateway_start(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.gateway_start().await.map_err(err)
}

#[tauri::command]
pub async fn gateway_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.gateway_stop().await.map_err(err)
}

// ------------------------------------------------------------------ chat --

pub const CHAT_EVENT: &str = "chat:event";

#[tauri::command]
pub async fn chat_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    state.supervisor.chat_sessions().await.map_err(err)
}

#[tauri::command]
pub async fn chat_create_session(
    state: State<'_, AppState>,
    title: Option<String>,
) -> Result<SessionSummary, String> {
    state
        .supervisor
        .chat_create_session(title.as_deref())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn chat_delete_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    state
        .supervisor
        .chat_delete_session(&session_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn chat_messages(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state
        .supervisor
        .chat_messages(&session_id)
        .await
        .map_err(err)
}

#[tauri::command]
pub fn chat_send(
    state: State<'_, AppState>,
    session_id: String,
    message: String,
) -> Result<(), String> {
    let fut = state
        .supervisor
        .chat_send(&session_id, &message)
        .map_err(err)?;
    tauri::async_runtime::spawn(fut);
    Ok(())
}

#[tauri::command]
pub async fn chat_stop(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    state.supervisor.chat_stop(&session_id).await.map_err(err)
}

#[tauri::command]
pub async fn chat_approve(
    state: State<'_, AppState>,
    run_id: String,
    choice: String,
) -> Result<(), String> {
    state
        .supervisor
        .chat_approve(&run_id, &choice)
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
