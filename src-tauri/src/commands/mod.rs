//! Tauri commands callable from the UI. Every exported type is mirrored to
//! `src/types/generated/` by ts-rs (regenerated on `cargo test`). All real
//! work lives in `hermes_core::supervisor`, which the control API shares.

use hermes_core::chat::{ChatMessage, SessionSummary};
use hermes_core::envkeys::{EnvKey, EnvPreset};
use hermes_core::freescout::TicketsSnapshot;
use hermes_core::gateway::Gateway;
use hermes_core::quick::{QuickCommandInfo, QuickMatch};
use hermes_core::setup::{SetupOptions, SetupPlan};
use hermes_core::status::StatusSnapshot;
use hermes_core::supervisor_quick::QuickRunResult;
use hermes_core::voice::VoiceStatus;
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

// ----------------------------------------------------------------- voice --

#[tauri::command]
pub async fn voice_status(state: State<'_, AppState>) -> Result<VoiceStatus, String> {
    state.supervisor.voice_status().await.map_err(err)
}

/// Install voice dependencies and start the Hermes web server; progress
/// arrives as `setup:event`.
#[tauri::command]
pub fn voice_setup(state: State<'_, AppState>) -> Result<(), String> {
    let fut = state.supervisor.voice_setup().map_err(err)?;
    tauri::async_runtime::spawn(fut);
    Ok(())
}

/// `audio_base64` without the `data:` prefix; returns the transcript ("" = silence).
#[tauri::command]
pub async fn voice_transcribe(
    state: State<'_, AppState>,
    audio_base64: String,
    mime_type: Option<String>,
) -> Result<String, String> {
    use base64::Engine as _;
    let audio = base64::engine::general_purpose::STANDARD
        .decode(audio_base64.trim())
        .map_err(err)?;
    state
        .supervisor
        .voice_transcribe(&audio, mime_type.as_deref().unwrap_or("audio/wav"))
        .await
        .map_err(err)
}

/// Returns a `data:` URL with the synthesized speech.
#[tauri::command]
pub async fn voice_speak(state: State<'_, AppState>, text: String) -> Result<String, String> {
    state.supervisor.voice_speak(&text).await.map_err(err)
}

// ----------------------------------------------------------------- quick --

#[tauri::command]
pub fn quick_commands(state: State<'_, AppState>) -> Result<Vec<QuickCommandInfo>, String> {
    state.supervisor.quick_commands().map_err(err)
}

#[tauri::command]
pub fn quick_match(state: State<'_, AppState>, text: String) -> Result<Option<QuickMatch>, String> {
    state.supervisor.quick_match(&text).map_err(err)
}

#[tauri::command]
pub async fn quick_run(
    state: State<'_, AppState>,
    id: String,
    confirmed: Option<bool>,
    query: Option<String>,
) -> Result<QuickRunResult, String> {
    state
        .supervisor
        .quick_run(&id, confirmed.unwrap_or(false), query.as_deref())
        .await
        .map_err(err)
}

/// Copy a folder of `.wav` reactions (e.g. Luxify's `assets\Voices\Jarvis`)
/// into the app's voice pack.
#[tauri::command]
pub fn quick_import_voices(
    state: State<'_, AppState>,
    source_dir: String,
    name: Option<String>,
) -> Result<usize, String> {
    state
        .supervisor
        .quick_import_voices(
            std::path::Path::new(&source_dir),
            name.as_deref()
                .unwrap_or(hermes_core::supervisor_quick::DEFAULT_VOICE),
        )
        .map_err(err)
}

// ----------------------------------------------------------- permissions --

/// Last decisions of the permission plugin (JSON objects, newest last).
#[tauri::command]
pub fn permissions_log(
    state: State<'_, AppState>,
    tail: Option<usize>,
) -> Result<Vec<serde_json::Value>, String> {
    let path = hermes_core::plugins::log_path(&state.supervisor.ctx.layout);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(err)?;
    let lines: Vec<&str> = text.lines().collect();
    let n = tail.unwrap_or(200).clamp(1, 5000);
    let start = lines.len().saturating_sub(n);
    Ok(lines[start..]
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

#[tauri::command]
pub fn permissions_policy_get(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let path = hermes_core::plugins::policy_path(&state.supervisor.ctx.layout);
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let text = std::fs::read_to_string(&path).map_err(err)?;
    serde_json::from_str(&text).map_err(err)
}

#[tauri::command]
pub fn permissions_policy_set(
    state: State<'_, AppState>,
    policy: serde_json::Value,
) -> Result<(), String> {
    if !policy.is_object() {
        return Err("policy must be a JSON object".into());
    }
    let path = hermes_core::plugins::policy_path(&state.supervisor.ctx.layout);
    let text = serde_json::to_string_pretty(&policy).map_err(err)?;
    hermes_core::control::write_with_backup(&path, &text).map_err(err)
}

// ------------------------------------------------------- API keys (.env) --

#[tauri::command]
pub fn env_keys_list(state: State<'_, AppState>) -> Result<Vec<EnvKey>, String> {
    hermes_core::envkeys::list(&state.supervisor.ctx.layout).map_err(err)
}

#[tauri::command]
pub fn env_keys_presets() -> Vec<EnvPreset> {
    hermes_core::envkeys::presets()
}

#[tauri::command]
pub fn env_keys_set(state: State<'_, AppState>, name: String, value: String) -> Result<(), String> {
    hermes_core::envkeys::set(&state.supervisor.ctx.layout, &name, &value).map_err(err)
}

#[tauri::command]
pub fn env_keys_remove(state: State<'_, AppState>, name: String) -> Result<(), String> {
    hermes_core::envkeys::remove(&state.supervisor.ctx.layout, &name).map_err(err)
}

#[tauri::command]
pub async fn gateway_restart(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.gateway_restart().await.map_err(err)
}

// ------------------------------------------------------------- tickets --

pub const TICKETS_WINDOW: &str = "tickets";

#[tauri::command]
pub async fn tickets_fetch(state: State<'_, AppState>) -> Result<TicketsSnapshot, String> {
    state.supervisor.freescout_tickets().await.map_err(err)
}

/// Small always-on-top window with the ticket list; focused if already open.
/// `async` on purpose: window creation then runs off the main thread and the
/// main thread stays free to serve the webview (a sync command blocked it and
/// the new window stayed blank on Windows, v0.13.0).
#[tauri::command]
pub async fn tickets_window_open(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;
    log::info!("tickets window: open requested");
    if let Some(w) = app.get_webview_window(TICKETS_WINDOW) {
        let _ = w.unminimize();
        w.show().map_err(err)?;
        w.set_focus().map_err(err)?;
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        TICKETS_WINDOW,
        tauri::WebviewUrl::App("index.html#/tickets".into()),
    )
    .title("Мои тикеты")
    .inner_size(480.0, 680.0)
    .min_inner_size(360.0, 400.0)
    .always_on_top(true)
    .build()
    .map_err(err)?;
    Ok(())
}

pub const HUD_WINDOW: &str = "hud";

/// Frameless, transparent, always-on-top reactor in the bottom-right corner.
#[tauri::command]
pub async fn hud_window_open(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;
    log::info!("hud window: open requested");
    if let Some(w) = app.get_webview_window(HUD_WINDOW) {
        w.show().map_err(err)?;
        return Ok(());
    }
    let size = 220.0;
    let w = tauri::WebviewWindowBuilder::new(
        &app,
        HUD_WINDOW,
        tauri::WebviewUrl::App("index.html#/hud".into()),
    )
    .title("Джарвис")
    .inner_size(size, size)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .build()
    .map_err(err)?;
    // Bottom-right of the primary monitor, above the taskbar.
    if let Ok(Some(m)) = w.primary_monitor() {
        let scale = m.scale_factor();
        let area = m.work_area();
        let x = (area.position.x as f64 + area.size.width as f64) / scale - size - 16.0;
        let y = (area.position.y as f64 + area.size.height as f64) / scale - size - 16.0;
        let _ = w.set_position(tauri::LogicalPosition::new(x, y));
    }
    log::info!("hud window: created");
    Ok(())
}

#[tauri::command]
pub async fn hud_window_close(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(HUD_WINDOW) {
        w.close().map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    hermes_core::quick::open_url(&url).await.map_err(err)
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
