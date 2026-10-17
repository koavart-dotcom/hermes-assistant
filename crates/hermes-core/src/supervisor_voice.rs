//! Voice side of the [`Supervisor`]: the Hermes web server as a supervised
//! child, one-off dependency setup (streamed through the setup event log),
//! transcription and speech synthesis for the UI and the control API.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::events::SetupEvent;
use crate::supervisor::Supervisor;
use crate::voice::{VoiceServer, VoiceStatus};
use crate::{CoreError, Result};

/// Reaction clips (voice pack paths) played while the model works on a voice question.
const ACK_CLIPS: &[&str] = &[
    "Подтверждение_Ответ/Вас понял, сэр",
    "Подтверждение_Ответ/Принято, сэр",
];

impl Supervisor {
    pub fn voice_server(&self) -> Result<VoiceServer> {
        VoiceServer::new(self.ctx.layout.clone())
    }

    pub async fn voice_status(&self) -> Result<VoiceStatus> {
        let vs = self.voice_server()?;
        let sp = crate::models::current_speech(&self.ctx.layout);
        Ok(VoiceStatus {
            hermes_installed: self.ctx.layout.bootstrap_marker().exists(),
            deps_ready: vs.deps_ready(),
            server_running: vs.is_running().await,
            stt_provider: format!("{} {}", sp.stt_provider, sp.stt_model),
            tts_provider: sp.tts_provider,
            tts_voice: sp.tts_voice,
        })
    }

    pub fn voice_setup_running(&self) -> bool {
        self.voice_setup
            .lock()
            .map(|s| s.is_some())
            .unwrap_or_else(|e| e.into_inner().is_some())
    }

    /// Install voice dependencies and configure Hermes, then start the web
    /// server. Progress goes to the setup event stream (`record`), so the UI
    /// and `hermesctl events` show it. Returns the future for the caller to
    /// spawn.
    pub fn voice_setup(self: &Arc<Self>) -> Result<impl Future<Output = ()> + Send + 'static> {
        if !self.ctx.layout.bootstrap_marker().exists() {
            return Err(CoreError::Other("Hermes is not installed yet".into()));
        }
        let cancel = {
            let mut slot = self.voice_setup.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_some() {
                return Err(CoreError::Other("voice setup is already running".into()));
            }
            let c = CancellationToken::new();
            *slot = Some(c.clone());
            c
        };
        let sup = self.clone();
        Ok(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<SetupEvent>();
            let pump = {
                let sup = sup.clone();
                async move {
                    while let Some(ev) = rx.recv().await {
                        sup.record(ev);
                    }
                }
            };
            let run = {
                let sup = sup.clone();
                async move {
                    let result = async {
                        let vs = sup.voice_server()?;
                        vs.install_deps(&tx, &cancel).await?;
                        if let Err(e) = crate::voice::ensure_cuda_dlls(&sup.ctx.layout) {
                            log::warn!("voice: CUDA DLLs for Whisper: {e}");
                        }
                        let _ = tx.send(SetupEvent::Log {
                            line: "voice: starting the Hermes web server".into(),
                        });
                        if let Some(child) = vs.start().await? {
                            let mut c = sup
                                .dashboard_child
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            *c = Some(child);
                        }
                        Ok::<(), CoreError>(())
                    }
                    .await;
                    let _ = match result {
                        Ok(()) => tx.send(SetupEvent::Log {
                            line: "voice: ready".into(),
                        }),
                        Err(e) => tx.send(SetupEvent::Failed {
                            message: format!("voice setup failed: {e}"),
                        }),
                    };
                    drop(tx);
                }
            };
            tokio::join!(run, pump);
            let mut slot = sup.voice_setup.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
        })
    }

    /// At app start (and on demand): bring the web server up when voice was
    /// set up before. No-op otherwise.
    pub async fn voice_ensure_server(&self) -> Result<bool> {
        let vs = self.voice_server()?;
        if !vs.deps_ready() {
            return Ok(false);
        }
        if let Err(e) = crate::voice::ensure_cuda_dlls(&self.ctx.layout) {
            log::warn!("voice: CUDA DLLs for Whisper: {e}");
        }
        crate::voice::jarvis_tts_warm(&self.ctx.layout).await;
        if let Some(child) = vs.start().await? {
            let mut c = self
                .dashboard_child
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *c = Some(child);
        }
        Ok(true)
    }

    /// Stop our web server (so it reloads the Hermes environment, e.g. after
    /// `hermes pm install`) and start it again.
    pub async fn voice_restart_server(&self) -> Result<()> {
        let vs = self.voice_server()?;
        if let Err(e) = crate::voice::ensure_cuda_dlls(&self.ctx.layout) {
            log::warn!("voice: CUDA DLLs for Whisper: {e}");
        }
        {
            let mut c = self
                .dashboard_child
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(child) = c.as_mut() {
                let _ = child.start_kill();
            }
            *c = None;
        }
        vs.stop_owned().await?;
        if let Some(child) = vs.start().await? {
            let mut c = self
                .dashboard_child
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *c = Some(child);
        }
        Ok(())
    }

    pub async fn voice_transcribe(&self, audio: &[u8], mime: &str) -> Result<String> {
        let vs = self.voice_server()?;
        if !vs.is_running().await {
            self.voice_ensure_server().await?;
        }
        vs.transcribe(audio, mime).await
    }

    /// Right after a voice question goes to the model: wake the Jarvis voice
    /// server (it unloads itself after a quiet spell, loading takes 10–60 s and
    /// overlaps with the model's thinking) and play a short reaction clip.
    /// Returns once the clip has finished.
    pub async fn voice_ack(&self) -> Result<()> {
        let layout = self.ctx.layout.clone();
        tokio::spawn(async move { crate::voice::jarvis_tts_warm(&layout).await });
        let prefs = self.prefs();
        if prefs.silent {
            return Ok(());
        }
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        let clip = ACK_CLIPS[nanos % ACK_CLIPS.len()];
        match crate::quick::clip_path(&self.ctx.layout, &self.quick_voice(), clip) {
            Some(path) => crate::quick::play_clip(&path, prefs.voice_volume).await,
            None => Ok(()),
        }
    }

    pub async fn voice_speak(&self, text: &str) -> Result<String> {
        let vs = self.voice_server()?;
        if !vs.is_running().await {
            self.voice_ensure_server().await?;
        }
        vs.speak(text).await
    }
}
