//! Process-wide state: the supervisor (paths, pinned Hermes revision, running
//! setup, Ollama child, event stream) shared by Tauri commands and the
//! control API.

use std::path::PathBuf;
use std::sync::Arc;

use hermes_core::Layout;
use hermes_core::bootstrap::Pin;
use hermes_core::setup::SetupContext;
use hermes_core::supervisor::Supervisor;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

pub struct AppState {
    pub supervisor: Arc<Supervisor>,
}

impl AppState {
    pub fn init(app: &AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
        let script: PathBuf = app
            .path()
            .resolve("resources/hermes/install.ps1", BaseDirectory::Resource)?;
        let pin_path = app
            .path()
            .resolve("resources/hermes/pin.json", BaseDirectory::Resource)?;
        let pin: Pin = serde_json::from_str(&std::fs::read_to_string(&pin_path)?)?;
        let plugins_dir: PathBuf = app
            .path()
            .resolve("resources/hermes-plugins", BaseDirectory::Resource)?;
        let layout = Layout::detect();
        log::info!("hermes home: {}", layout.hermes_home.display());
        log::info!("models dir: {}", layout.models_dir.display());
        log::info!("app data dir: {}", layout.app_data_dir.display());
        Ok(AppState {
            supervisor: Supervisor::new(Arc::new(SetupContext {
                layout,
                script,
                pin,
                plugins_dir,
            })),
        })
    }
}
