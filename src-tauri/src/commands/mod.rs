//! Tauri commands callable from the UI. Every exported type is mirrored to
//! `src/types/generated/` by ts-rs (regenerated on `cargo test`).

use serde::Serialize;
use ts_rs::TS;

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

#[tauri::command]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    let pkg = app.package_info();
    AppInfo {
        name: pkg.name.clone(),
        version: pkg.version.to_string(),
        // Stage 2 replaces this with the real supervisor state.
        hermes_state: HermesState::NotInstalled,
    }
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
