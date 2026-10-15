//! Hermes Agent bootstrap through its own `install.ps1` stage protocol
//! (protocol version 1): `-Manifest` lists the stages, then each stage runs
//! as `-Stage NAME -NonInteractive -Json` and prints one JSON frame.
//! This mirrors `apps/bootstrap-installer` (Hermes-Setup, Tauri, MIT).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::cli::{hermes_env, powershell_script, run_streaming};
use crate::events::{EventSender, SetupEvent, emit};
use crate::{CoreError, Layout, Result};

/// Pinned Hermes revision shipped with the app (`resources/hermes/pin.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pin {
    pub version: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageInfo {
    pub name: String,
    pub title: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub needs_user_input: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub protocol_version: u32,
    pub stages: Vec<StageInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StageFrame {
    pub ok: bool,
    pub stage: String,
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

pub const SUPPORTED_PROTOCOL: u32 = 1;

/// Find the last stdout line that parses as `T` (the installer prints human
/// text before the JSON payload).
fn last_json<T: for<'de> Deserialize<'de>>(lines: &[String]) -> Option<T> {
    lines
        .iter()
        .rev()
        .filter(|l| l.trim_start().starts_with('{'))
        .find_map(|l| serde_json::from_str::<T>(l.trim()).ok())
}

pub fn parse_manifest(lines: &[String]) -> Result<Manifest> {
    let m: Manifest = last_json(lines)
        .ok_or_else(|| CoreError::Protocol("install.ps1 -Manifest produced no JSON".into()))?;
    if m.protocol_version != SUPPORTED_PROTOCOL {
        return Err(CoreError::Protocol(format!(
            "unsupported install protocol {} (expected {SUPPORTED_PROTOCOL})",
            m.protocol_version
        )));
    }
    Ok(m)
}

pub fn parse_frame(lines: &[String], stage: &str) -> Result<StageFrame> {
    last_json::<StageFrame>(lines)
        .filter(|f| f.stage == stage)
        .ok_or_else(|| CoreError::Protocol(format!("stage `{stage}` produced no JSON result frame")))
}

pub struct Installer {
    pub script: PathBuf,
    pub pin: Pin,
    pub layout: Layout,
}

impl Installer {
    pub fn is_installed(&self) -> bool {
        self.layout.bootstrap_marker().exists()
    }

    fn base_command(&self) -> tokio::process::Command {
        let mut cmd = powershell_script(&self.script);
        hermes_env(&mut cmd, &self.layout);
        cmd.env("HERMES_INSTALL_NONINTERACTIVE", "1");
        cmd
    }

    pub async fn manifest(&self, events: &EventSender, cancel: &CancellationToken) -> Result<Manifest> {
        let mut cmd = self.base_command();
        cmd.arg("-Manifest");
        let out = run_streaming(cmd, "install", Some(events), cancel).await?;
        if !out.success {
            return Err(CoreError::Protocol(format!("install.ps1 -Manifest failed: {}", out.status)));
        }
        parse_manifest(&out.stdout)
    }

    /// Run every non-interactive stage in manifest order. Stages that need
    /// user input (`setup`, `gateway`) are skipped: our shell configures
    /// Hermes itself afterwards.
    pub async fn install(&self, events: &EventSender, cancel: &CancellationToken) -> Result<()> {
        if !cfg!(windows) {
            return Err(CoreError::Other("Hermes bootstrap is implemented for Windows only".into()));
        }
        let manifest = self.manifest(events, cancel).await?;
        for stage in &manifest.stages {
            if stage.needs_user_input {
                emit(events, SetupEvent::StageFinished {
                    stage: stage.name.clone(),
                    ok: true,
                    skipped: true,
                    reason: Some("configured by Hermes shell".into()),
                });
                continue;
            }
            emit(events, SetupEvent::StageStarted { stage: stage.name.clone(), title: stage.title.clone() });
            let mut cmd = self.base_command();
            cmd.args(["-Stage", &stage.name, "-NonInteractive", "-Json", "-Commit", &self.pin.commit]);
            let out = run_streaming(cmd, "install", Some(events), cancel).await?;
            let frame = parse_frame(&out.stdout, &stage.name).or_else(|e| {
                if out.success { Err(e) } else {
                    Err(CoreError::StageFailed { stage: stage.name.clone(), reason: out.status.clone() })
                }
            })?;
            emit(events, SetupEvent::StageFinished {
                stage: frame.stage.clone(),
                ok: frame.ok,
                skipped: frame.skipped,
                reason: frame.reason.clone(),
            });
            if !frame.ok {
                return Err(CoreError::StageFailed {
                    stage: stage.name.clone(),
                    reason: frame.reason.unwrap_or_else(|| "installer reported failure".into()),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_manifest_after_noise() {
        let out = lines(&[
            "[hermes] resolving paths",
            r#"{"protocol_version":1,"stages":[{"name":"prerequisites","title":"System prerequisites","category":"runtime","needs_user_input":false},{"name":"setup","title":"Configure","category":"configuration","needs_user_input":true}]}"#,
        ]);
        let m = parse_manifest(&out).unwrap();
        assert_eq!(m.stages.len(), 2);
        assert!(m.stages[1].needs_user_input);
    }

    #[test]
    fn rejects_other_protocol() {
        let out = lines(&[r#"{"protocol_version":2,"stages":[]}"#]);
        assert!(matches!(parse_manifest(&out), Err(CoreError::Protocol(_))));
    }

    #[test]
    fn parses_last_frame_for_stage() {
        let out = lines(&[
            r#"{"ok":true,"stage":"prerequisites","skipped":false}"#,
            "installing...",
            r#"{"ok":false,"stage":"venv","skipped":false,"reason":"uv failed"}"#,
        ]);
        let f = parse_frame(&out, "venv").unwrap();
        assert!(!f.ok);
        assert_eq!(f.reason.as_deref(), Some("uv failed"));
        assert!(parse_frame(&out, "repository").is_err());
    }
}
