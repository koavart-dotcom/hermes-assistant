//! A snapshot of everything the Status screen shows.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::gateway::Gateway;
use crate::ollama::Ollama;
use crate::{Layout, Result};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct StatusSnapshot {
    pub hermes_installed: bool,
    pub hermes_version: String,
    pub gateway_running: bool,
    pub ollama_installed: bool,
    pub ollama_running: bool,
    pub models: Vec<String>,
    pub hermes_home: String,
    pub models_dir: String,
}

pub async fn snapshot(layout: &Layout, hermes_version: &str, ollama: &Ollama, gateway: &Gateway) -> Result<StatusSnapshot> {
    let ollama_running = ollama.is_running().await;
    let models = if ollama_running { ollama.models().await.unwrap_or_default() } else { Vec::new() };
    Ok(StatusSnapshot {
        hermes_installed: layout.bootstrap_marker().exists(),
        hermes_version: hermes_version.to_string(),
        gateway_running: gateway.is_healthy().await,
        ollama_installed: ollama.is_installed(),
        ollama_running,
        models,
        hermes_home: layout.hermes_home.display().to_string(),
        models_dir: layout.models_dir.display().to_string(),
    })
}
