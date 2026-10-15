//! Progress events streamed from the setup pipeline to the UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One step of the first-run setup as shown in the wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum SetupStep {
    Hermes,
    Config,
    Ollama,
    Model,
    Gateway,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum SetupEvent {
    /// A top-level step started.
    StepStarted { step: SetupStep },
    /// A Hermes install.ps1 stage started.
    StageStarted { stage: String, title: String },
    /// A Hermes install.ps1 stage finished.
    StageFinished {
        stage: String,
        ok: bool,
        skipped: bool,
        reason: Option<String>,
    },
    /// Byte-level progress of a download or model pull.
    Progress {
        label: String,
        received: u64,
        total: Option<u64>,
    },
    /// A raw line from a child process, for the log pane.
    Log { line: String },
    /// A top-level step finished.
    StepFinished { step: SetupStep },
    /// The whole setup finished successfully.
    Done,
    /// The setup stopped with an error.
    Failed { message: String },
}

pub type EventSender = tokio::sync::mpsc::UnboundedSender<SetupEvent>;

/// Send an event, ignoring a closed receiver (the UI may have gone away).
pub fn emit(tx: &EventSender, event: SetupEvent) {
    let _ = tx.send(event);
}
