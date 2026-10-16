//! Voice through the Hermes **web server** (`hermes dashboard`): it is the
//! only Hermes surface with audio endpoints in v0.22.2 —
//! `POST /api/audio/transcribe` (`{data_url}` → `{transcript}`) and
//! `POST /api/audio/speak` (`{text}` → `{data_url}`), both behind the
//! `X-Hermes-Session-Token` header. We run it headless on loopback as a
//! second supervised process with a token we mint
//! (`HERMES_DASHBOARD_SESSION_TOKEN`). STT = local Faster-Whisper (`hermes pm
//! install --extra voice`, model downloads on first use), TTS = Edge TTS with
//! a Russian voice (free, needs internet); Piper comes later.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::cli::{command_for, hermes_env, hide_console, run_hermes};
use crate::events::{EventSender, SetupEvent, emit};
use crate::{CoreError, Layout, Result};

pub const DASHBOARD_PORT: u16 = 8644;
pub const TOKEN_HEADER: &str = "X-Hermes-Session-Token";
/// Written after `hermes pm install --extra voice` succeeded once.
pub const READY_MARKER: &str = "voice-ready";

/// What the UI shows on the Voice page.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct VoiceStatus {
    pub hermes_installed: bool,
    pub deps_ready: bool,
    pub server_running: bool,
    pub stt_provider: String,
    pub tts_provider: String,
    pub tts_voice: String,
}

pub fn ready_marker(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join(READY_MARKER)
}

pub fn token_path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("dashboard.token")
}

pub fn log_file(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("logs").join("dashboard.log")
}

/// Session token for the web server, created on first use.
pub fn token(layout: &Layout) -> Result<String> {
    let path = token_path(layout);
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if s.len() >= 32 {
            return Ok(s);
        }
    }
    let t = crate::secrets::generate_key()?;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, &t)?;
    Ok(t)
}

/// The Hermes config keys our shell owns for voice (Russian, local STT).
pub fn settings() -> Vec<(String, String)> {
    vec![
        ("stt.enabled".into(), "true".into()),
        ("stt.provider".into(), "local".into()),
        ("stt.local.model".into(), "small".into()),
        ("stt.local.language".into(), "ru".into()),
        // Piper: offline, Russian voices auto-download from rhasspy/piper-voices.
        // Edge TTS (the Hermes default) answered 403 on the PC on 16.10.2026 —
        // Microsoft locked the free endpoint for older clients.
        ("tts.provider".into(), "piper".into()),
        ("tts.piper.voice".into(), "ru_RU-dmitri-medium".into()),
    ]
}

#[derive(Debug, Clone)]
pub struct VoiceServer {
    pub layout: Layout,
    client: reqwest::Client,
    token: String,
}

impl VoiceServer {
    pub fn new(layout: Layout) -> Result<VoiceServer> {
        let token = token(&layout)?;
        Ok(VoiceServer {
            layout,
            client: reqwest::Client::new(),
            token,
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{DASHBOARD_PORT}")
    }

    pub fn deps_ready(&self) -> bool {
        ready_marker(&self.layout).exists()
    }

    /// `GET /api/audio/voice-config` with our token answers 200 only when
    /// our server (same token) is up.
    pub async fn is_running(&self) -> bool {
        self.client
            .get(format!("{}/api/audio/voice-config", self.base_url()))
            .header(TOKEN_HEADER, &self.token)
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Install Faster-Whisper + audio deps into the Hermes environment and
    /// apply the Russian STT/TTS settings. Idempotent; long on first run.
    pub async fn install_deps(
        &self,
        events: &EventSender,
        cancel: &CancellationToken,
    ) -> Result<()> {
        emit(
            events,
            SetupEvent::Log {
                line: "voice: hermes pm install --extra voice (Faster-Whisper, audio)".into(),
            },
        );
        run_hermes(
            &self.layout,
            &["pm", "install", "--extra", "voice"],
            Some(events),
            cancel,
        )
        .await?;
        emit(
            events,
            SetupEvent::Log {
                line: "voice: hermes pm install --extra piper (offline TTS)".into(),
            },
        );
        run_hermes(
            &self.layout,
            &["pm", "install", "--extra", "piper"],
            Some(events),
            cancel,
        )
        .await?;
        for (key, value) in settings() {
            emit(
                events,
                SetupEvent::Log {
                    line: format!("hermes config set {key} {value}"),
                },
            );
            run_hermes(&self.layout, &["config", "set", &key, &value], None, cancel).await?;
        }
        let marker = ready_marker(&self.layout);
        if let Some(p) = marker.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(&marker, "ok")?;
        Ok(())
    }

    /// Start `hermes dashboard --no-open` on loopback unless it already
    /// answers. The first start may lazy-install fastapi/uvicorn (minutes).
    pub async fn start(&self) -> Result<Option<Child>> {
        if self.is_running().await {
            return Ok(None);
        }
        let log_path = log_file(&self.layout);
        if let Some(p) = log_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let log_err = log.try_clone()?;
        let mut cmd: Command = command_for(&self.layout.hermes_cli());
        cmd.args([
            "dashboard",
            "--no-open",
            "--host",
            "127.0.0.1",
            "--port",
            &DASHBOARD_PORT.to_string(),
        ]);
        hermes_env(&mut cmd, &self.layout);
        cmd.env("HERMES_DASHBOARD_SESSION_TOKEN", &self.token)
            .env("HERMES_DESKTOP", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .kill_on_drop(false);
        hide_console(&mut cmd);
        let mut child = cmd.spawn()?;
        for _ in 0..600 {
            if let Some(status) = child.try_wait()? {
                return Err(CoreError::Other(format!(
                    "hermes dashboard exited with {status}; see {}",
                    log_path.display()
                )));
            }
            if self.is_running().await {
                return Ok(Some(child));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(CoreError::Other(
            "hermes dashboard did not become ready in 300 s".into(),
        ))
    }

    /// Kill `hermes dashboard` processes we started (Windows: python with
    /// `dashboard` and our port on the command line) and wait until the port
    /// stops answering.
    pub async fn stop_owned(&self) -> Result<()> {
        if !cfg!(windows) {
            return Ok(());
        }
        let script = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.Name -match 'python' -and $_.CommandLine -match 'dashboard' -and $_.CommandLine -match '{DASHBOARD_PORT}' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}"
        );
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut cmd);
        let _ = cmd.status().await;
        for _ in 0..40 {
            if !self.is_running().await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err(CoreError::Other(
            "hermes dashboard still answers after stopping".into(),
        ))
    }

    /// Transcribe a recording (`mime` like `audio/wav`); empty string = silence.
    pub async fn transcribe(&self, audio: &[u8], mime: &str) -> Result<String> {
        use base64::Engine as _;
        let data_url = format!(
            "data:{mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(audio)
        );
        let resp = self
            .client
            .post(format!("{}/api/audio/transcribe", self.base_url()))
            .header(TOKEN_HEADER, &self.token)
            .timeout(Duration::from_secs(300))
            .json(&json!({ "data_url": data_url, "mime_type": mime }))
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(CoreError::Other(format!(
                "transcribe: HTTP {status}: {}",
                body.get("detail").and_then(Value::as_str).unwrap_or("")
            )));
        }
        Ok(body
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string())
    }

    /// Synthesize `text`; returns a `data:<mime>;base64,...` URL the webview
    /// can play directly.
    pub async fn speak(&self, text: &str) -> Result<String> {
        let resp = self
            .client
            .post(format!("{}/api/audio/speak", self.base_url()))
            .header(TOKEN_HEADER, &self.token)
            .timeout(Duration::from_secs(120))
            .json(&json!({ "text": text }))
            .send()
            .await?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(CoreError::Other(format!(
                "speak: HTTP {status}: {}",
                body.get("detail").and_then(Value::as_str).unwrap_or("")
            )));
        }
        body.get("data_url")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| CoreError::Protocol("speak: no data_url in response".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_make_russian_local_voice() {
        let s = settings();
        let get = |k: &str| s.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("stt.provider"), Some("local"));
        assert_eq!(get("stt.local.language"), Some("ru"));
        assert_eq!(get("tts.provider"), Some("piper"));
        assert!(get("tts.piper.voice").unwrap().starts_with("ru_RU"));
    }

    #[test]
    fn token_is_created_and_reused() {
        let root = std::env::temp_dir().join(format!("hermes-voice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        let a = token(&layout).unwrap();
        let b = token(&layout).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        let vs = VoiceServer::new(layout.clone()).unwrap();
        assert!(!vs.deps_ready());
        assert_eq!(vs.base_url(), "http://127.0.0.1:8644");
        let _ = std::fs::remove_dir_all(&root);
    }
}
