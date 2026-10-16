//! In-app updates through tauri-plugin-updater: `latest.json` of the newest
//! GitHub release (signed by the key in `plugins.updater.pubkey`), downloaded
//! and installed on request, then relaunch. The repository is private, so a
//! read-only GitHub token (stored by the control API, see
//! `hermes_core::secrets::github_token`) is sent with every request.

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::UpdaterExt;
use ts_rs::TS;

use crate::state::AppState;

pub const UPDATE_EVENT: &str = "update:progress";

/// Result of an update check shown to the user.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct UpdateInfo {
    pub current: String,
    pub available: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn updater(app: &AppHandle, state: &AppState) -> Result<tauri_plugin_updater::Updater, String> {
    let mut builder = app.updater_builder();
    if let Some(token) = hermes_core::secrets::github_token(&state.supervisor.ctx.layout) {
        builder = builder
            .header("Authorization", format!("Bearer {token}"))
            .map_err(err)?;
    }
    builder.build().map_err(err)
}

#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let update = updater(&app, &state)?.check().await.map_err(err)?;
    Ok(UpdateInfo {
        current,
        available: update.as_ref().map(|u| u.version.clone()),
        notes: update.and_then(|u| u.body),
    })
}

/// Download, install and relaunch. Emits `update:progress` while downloading.
#[tauri::command]
pub async fn update_install(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let Some(update) = updater(&app, &state)?.check().await.map_err(err)? else {
        return Err("no update available".into());
    };
    let mut downloaded: u64 = 0;
    let progress_app = app.clone();
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress_app.emit(UPDATE_EVENT, Progress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(err)?;
    log::info!("update installed, relaunching");
    app.restart();
}
