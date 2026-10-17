//! The first-run pipeline: Hermes → config → Ollama → model → gateway.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::bootstrap::{Installer, Pin};
use crate::events::{EventSender, SetupEvent, SetupStep, emit};
use crate::gateway::Gateway;
use crate::ollama::{OLLAMA_VERSION, OLLAMA_ZIP_BYTES, Ollama};
use crate::{Layout, Result, hermes_config, secrets};

/// A local model the wizard offers. Sizes from ollama.com (15.10.2026).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ModelChoice {
    pub tag: String,
    pub label: String,
    pub bytes: u64,
    pub note: String,
}

pub fn known_models() -> Vec<ModelChoice> {
    vec![
        ModelChoice {
            tag: "qwen3.5".into(),
            label: "Qwen 3.5 (по умолчанию)".into(),
            bytes: 6_600_000_000,
            note: "Целиком в 12 ГБ видеопамяти, хороший tool calling".into(),
        },
        ModelChoice {
            tag: "gemma4:12b-it-qat".into(),
            label: "Gemma 4 12B".into(),
            bytes: 7_200_000_000,
            note: "Vision и надёжные вызовы инструментов".into(),
        },
        ModelChoice {
            tag: "qwen3:14b".into(),
            label: "Qwen 3 14B".into(),
            bytes: 9_300_000_000,
            note: "Сильнее, но контекст меньше: 9,3 ГБ из 12".into(),
        },
    ]
}

/// What the wizard shows before asking for consent.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct SetupPlan {
    pub hermes_version: String,
    pub hermes_commit: String,
    pub hermes_installed: bool,
    pub ollama_version: String,
    pub ollama_installed: bool,
    pub ollama_zip_bytes: u64,
    pub models: Vec<ModelChoice>,
    pub default_model: String,
    pub hermes_home: String,
    pub ollama_dir: String,
    pub models_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct SetupOptions {
    pub model: String,
    /// The user agreed to the downloads listed in the plan.
    pub consent: bool,
    /// Register the gateway to start at login.
    pub autostart: bool,
}

pub struct SetupContext {
    pub layout: Layout,
    pub script: PathBuf,
    pub pin: Pin,
    /// Bundled Hermes plugins (`resources/hermes-plugins/`), one dir per plugin.
    pub plugins_dir: PathBuf,
    /// Bundled quick-command pack (`resources/hermes-quick-commands/default.ru.json`).
    pub quick_pack: PathBuf,
    /// Bundled Hermes skills (`resources/hermes-skills/`), one dir per skill.
    pub skills_dir: PathBuf,
}

impl SetupContext {
    pub fn plan(&self) -> SetupPlan {
        let ollama = Ollama::new(self.layout.clone());
        SetupPlan {
            hermes_version: self.pin.version.clone(),
            hermes_commit: self.pin.commit.clone(),
            hermes_installed: self.layout.bootstrap_marker().exists(),
            ollama_version: OLLAMA_VERSION.into(),
            ollama_installed: ollama.is_installed(),
            ollama_zip_bytes: OLLAMA_ZIP_BYTES,
            models: known_models(),
            default_model: "qwen3.5".into(),
            hermes_home: self.layout.hermes_home.display().to_string(),
            ollama_dir: self.layout.ollama_dir.display().to_string(),
            models_dir: self.layout.models_dir.display().to_string(),
        }
    }
}

/// Run the whole pipeline. Errors are also reported as `SetupEvent::Failed`.
/// Returns the Ollama child process when we started one.
pub async fn run(
    ctx: &SetupContext,
    opts: &SetupOptions,
    events: &EventSender,
    cancel: &CancellationToken,
) -> Result<Option<tokio::process::Child>> {
    match run_inner(ctx, opts, events, cancel).await {
        Ok(child) => {
            emit(events, SetupEvent::Done);
            Ok(child)
        }
        Err(e) => {
            emit(
                events,
                SetupEvent::Failed {
                    message: e.to_string(),
                },
            );
            Err(e)
        }
    }
}

async fn run_inner(
    ctx: &SetupContext,
    opts: &SetupOptions,
    events: &EventSender,
    cancel: &CancellationToken,
) -> Result<Option<tokio::process::Child>> {
    if !opts.consent {
        return Err(crate::CoreError::Other(
            "downloads were not approved".into(),
        ));
    }

    // 1. Hermes Agent.
    emit(
        events,
        SetupEvent::StepStarted {
            step: SetupStep::Hermes,
        },
    );
    let installer = Installer {
        script: ctx.script.clone(),
        pin: ctx.pin.clone(),
        layout: ctx.layout.clone(),
    };
    if installer.is_installed() {
        emit(
            events,
            SetupEvent::Log {
                line: "Hermes Agent already installed".into(),
            },
        );
    } else {
        installer.install(events, cancel).await?;
    }
    emit(
        events,
        SetupEvent::StepFinished {
            step: SetupStep::Hermes,
        },
    );

    // 2. Configuration + API key.
    emit(
        events,
        SetupEvent::StepStarted {
            step: SetupStep::Config,
        },
    );
    let api_key = secrets::api_server_key(&ctx.layout)?;
    // The Ollama port goes into config.yaml, so choose it before configuring.
    let mut ollama = Ollama::new(ctx.layout.clone());
    ollama.ensure_port(events).await?;
    hermes_config::apply(
        &ctx.layout,
        &opts.model,
        &api_key,
        &ollama.base_url(),
        events,
        cancel,
    )
    .await?;
    // Our plugins (permission levels, camera) ship with the app.
    crate::plugins::install_plugins(&ctx.layout, &ctx.plugins_dir, Some(events), cancel).await?;
    emit(
        events,
        SetupEvent::StepFinished {
            step: SetupStep::Config,
        },
    );

    // 3. Ollama sidecar.
    emit(
        events,
        SetupEvent::StepStarted {
            step: SetupStep::Ollama,
        },
    );
    ollama.ensure_installed(events, cancel).await?;
    let child = ollama.start().await?;
    emit(
        events,
        SetupEvent::StepFinished {
            step: SetupStep::Ollama,
        },
    );

    // 4. Model.
    emit(
        events,
        SetupEvent::StepStarted {
            step: SetupStep::Model,
        },
    );
    if ollama.has_model(&opts.model).await {
        emit(
            events,
            SetupEvent::Log {
                line: format!("model {} already present", opts.model),
            },
        );
    } else {
        ollama.pull(&opts.model, events, cancel).await?;
    }
    emit(
        events,
        SetupEvent::StepFinished {
            step: SetupStep::Model,
        },
    );

    // 5. Gateway.
    emit(
        events,
        SetupEvent::StepStarted {
            step: SetupStep::Gateway,
        },
    );
    let gateway = Gateway::new(ctx.layout.clone(), Some(api_key));
    if opts.autostart {
        gateway.install_autostart(Some(events), cancel).await?;
    }
    gateway.start(Some(events), cancel).await?;
    emit(
        events,
        SetupEvent::StepFinished {
            step: SetupStep::Gateway,
        },
    );

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_in_known_models() {
        let plan = SetupContext {
            layout: Layout::from_roots(std::path::Path::new("/lad"), std::path::Path::new("/data")),
            script: PathBuf::from("install.ps1"),
            plugins_dir: PathBuf::from("plugins"),
            quick_pack: PathBuf::from("quick.json"),
            skills_dir: PathBuf::from("skills"),
            pin: Pin {
                version: "v0".into(),
                commit: "abc".into(),
                branch: "main".into(),
            },
        }
        .plan();
        assert!(plan.models.iter().any(|m| m.tag == plan.default_model));
        assert!(!plan.hermes_installed);
    }
}
