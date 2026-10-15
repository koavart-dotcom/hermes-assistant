//! Process-wide state: paths, the pinned Hermes revision, the running setup
//! task and the Ollama child we own.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hermes_core::bootstrap::Pin;
use hermes_core::setup::SetupContext;
use hermes_core::Layout;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

pub struct AppState {
    pub ctx: Arc<SetupContext>,
    pub setup: Mutex<Option<RunningSetup>>,
    pub ollama_child: Mutex<Option<tokio::process::Child>>,
}

pub struct RunningSetup {
    pub cancel: CancellationToken,
}

impl AppState {
    pub fn init(app: &AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
        let script: PathBuf = app.path().resolve("resources/hermes/install.ps1", BaseDirectory::Resource)?;
        let pin_path = app.path().resolve("resources/hermes/pin.json", BaseDirectory::Resource)?;
        let pin: Pin = serde_json::from_str(&std::fs::read_to_string(&pin_path)?)?;
        let layout = Layout::detect();
        log::info!("hermes home: {}", layout.hermes_home.display());
        log::info!("models dir: {}", layout.models_dir.display());
        Ok(AppState {
            ctx: Arc::new(SetupContext { layout, script, pin }),
            setup: Mutex::new(None),
            ollama_child: Mutex::new(None),
        })
    }
}
