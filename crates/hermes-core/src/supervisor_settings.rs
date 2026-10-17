//! Settings side of the [`Supervisor`]: shell preferences, model choice,
//! speech (STT/TTS) choice, reaction voice packs and pulling Ollama models.

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::events::SetupEvent;
use crate::models::{self, ModelOption, ModelSettings, SpeechSettings};
use crate::ollama::Ollama;
use crate::prefs::{self, Prefs};
use crate::supervisor::Supervisor;
use crate::{CoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct VoicePackInfo {
    pub name: String,
    pub clips: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct VoicePacks {
    pub selected: String,
    pub installed: Vec<VoicePackInfo>,
    /// Luxify Assistant voices found on this PC that are not imported yet.
    pub importable: Vec<String>,
}

fn count_wavs(dir: &std::path::Path) -> usize {
    fn walk(d: &std::path::Path, n: &mut usize) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, n);
                } else if p
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| x.eq_ignore_ascii_case("wav"))
                {
                    *n += 1;
                }
            }
        }
    }
    let mut n = 0;
    walk(dir, &mut n);
    n
}

impl Supervisor {
    /// Build the morning briefing now (calendar, mail, Jira, helpdesk — whatever
    /// is configured); send it to Telegram when asked and configured.
    pub async fn digest_run(&self, telegram: bool) -> Result<crate::digest::DigestResult> {
        let layout = &self.ctx.layout;
        let now = chrono::Local::now();
        let cal_urls = crate::calendar::urls(layout);
        let txt = |e: CoreError| e.to_string();
        let tickets = async {
            match crate::freescout::config(layout) {
                Ok(Some(_)) => Some(self.freescout_tickets().await.map_err(txt)),
                Ok(None) => None,
                Err(e) => Some(Err(e.to_string())),
            }
        };
        let events = async {
            if cal_urls.is_empty() {
                None
            } else {
                Some(
                    crate::calendar::events_on(&cal_urls, now.date_naive())
                        .await
                        .map_err(txt),
                )
            }
        };
        let mail = async {
            match crate::mail::credentials(layout) {
                Some(c) => Some(crate::mail::summary(&c).await.map_err(txt)),
                None => None,
            }
        };
        let jira = async {
            match crate::jira::config(layout) {
                Some(c) => Some(crate::jira::my_open(&c).await.map_err(txt)),
                None => None,
            }
        };
        let (tickets, events, mail, jira) = tokio::join!(tickets, events, mail, jira);
        let b = crate::digest::Briefing {
            events,
            mail,
            jira,
            tickets,
        };
        if b.events.is_none() && b.mail.is_none() && b.jira.is_none() && b.tickets.is_none() {
            return Err(CoreError::Other(
                "Сводке не из чего собираться: задайте ключи календаря, почты, Jira или FreeScout в «Настройки → Интеграции и ключи»".into(),
            ));
        }
        let text = crate::digest::briefing_text(&b, &now.format("%d.%m.%Y").to_string());
        let spoken = crate::digest::briefing_spoken(&b);
        let (telegram_sent, telegram_error) = if telegram {
            match crate::digest::telegram_target(layout) {
                Some(t) => match crate::digest::send_telegram(&t, &text).await {
                    Ok(()) => (true, None),
                    Err(e) => (false, Some(e.to_string())),
                },
                None => (
                    false,
                    Some(
                        "Telegram не настроен: нужны TELEGRAM_BOT_TOKEN и TELEGRAM_CHAT_ID".into(),
                    ),
                ),
            }
        } else {
            (false, None)
        };
        let state = |o: bool, e: bool| match (o, e) {
            (false, _) => "-",
            (true, true) => "err",
            _ => "ok",
        };
        log::info!(
            "digest: calendar {} mail {} jira {} helpdesk {} telegram {}",
            state(
                b.events.is_some(),
                b.events.as_ref().is_some_and(|r| r.is_err())
            ),
            state(
                b.mail.is_some(),
                b.mail.as_ref().is_some_and(|r| r.is_err())
            ),
            state(
                b.jira.is_some(),
                b.jira.as_ref().is_some_and(|r| r.is_err())
            ),
            state(
                b.tickets.is_some(),
                b.tickets.as_ref().is_some_and(|r| r.is_err())
            ),
            telegram_sent
        );
        Ok(crate::digest::DigestResult {
            text,
            spoken,
            telegram_sent,
            telegram_error,
        })
    }

    pub fn prefs(&self) -> Prefs {
        prefs::load(&self.ctx.layout)
    }

    pub fn prefs_set(&self, p: &Prefs) -> Result<Prefs> {
        prefs::save(&self.ctx.layout, p)
    }

    // ---------------------------------------------------------------- model --

    pub async fn model_options(&self) -> Result<Vec<ModelOption>> {
        let ollama = Ollama::new(self.ctx.layout.clone());
        let local = if ollama.is_running().await {
            ollama.models().await.unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut opts = models::options(&local, &ollama.base_url());
        models::mark_ready(&mut opts, &self.ctx.layout);
        Ok(opts)
    }

    pub fn model_current(&self) -> ModelSettings {
        models::current_model(&self.ctx.layout)
    }

    /// Write the choice and restart the gateway so it takes effect.
    pub async fn model_apply(&self, s: &ModelSettings) -> Result<ModelSettings> {
        models::apply_model(&self.ctx.layout, s).await?;
        self.gateway_restart().await?;
        Ok(self.model_current())
    }

    /// Pull an Ollama model; progress goes to the setup event stream.
    pub fn model_pull(
        self: &Arc<Self>,
        tag: &str,
    ) -> Result<impl Future<Output = ()> + Send + 'static> {
        let tag = tag.trim().to_string();
        if tag.is_empty()
            || !tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c))
        {
            return Err(CoreError::Other(format!("недопустимое имя модели `{tag}`")));
        }
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
            let run = async move {
                let ollama = Ollama::new(sup.ctx.layout.clone());
                let _ = tx.send(SetupEvent::Log {
                    line: format!("ollama pull {tag}"),
                });
                let r = ollama.pull(&tag, &tx, &CancellationToken::new()).await;
                let _ = match r {
                    Ok(()) => tx.send(SetupEvent::Log {
                        line: format!("model: ready {tag}"),
                    }),
                    Err(e) => tx.send(SetupEvent::Failed {
                        message: format!("ollama pull {tag}: {e}"),
                    }),
                };
                drop(tx);
            };
            tokio::join!(run, pump);
        })
    }

    // --------------------------------------------------------------- speech --

    pub fn speech_current(&self) -> SpeechSettings {
        models::current_speech(&self.ctx.layout)
    }

    /// Write STT/TTS settings and restart the voice server.
    pub async fn speech_apply(&self, s: &SpeechSettings) -> Result<SpeechSettings> {
        models::apply_speech(&self.ctx.layout, s).await?;
        if let Err(e) = self.voice_restart_server().await {
            log::warn!("voice server restart after speech settings: {e}");
        }
        Ok(self.speech_current())
    }

    // ---------------------------------------------------------- voice packs --

    /// Luxify Assistant keeps its voices next to the command tree in
    /// `%LOCALAPPDATA%\LVA_CommandTreeProject\assets\Voices\<Name>`.
    pub fn luxify_voices_dir(&self) -> Option<PathBuf> {
        let local = self.ctx.layout.hermes_home.parent()?;
        let dir = local
            .join("LVA_CommandTreeProject")
            .join("assets")
            .join("Voices");
        dir.is_dir().then_some(dir)
    }

    pub fn voice_packs(&self) -> VoicePacks {
        let dir = crate::quick::voice_pack_dir(&self.ctx.layout);
        let mut installed: Vec<VoicePackInfo> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| VoicePackInfo {
                        name: e.file_name().to_string_lossy().to_string(),
                        clips: count_wavs(&e.path()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        installed.sort_by(|a, b| a.name.cmp(&b.name));
        let importable = self
            .luxify_voices_dir()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|rd| {
                let mut v: Vec<String> = rd
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .filter(|n| !installed.iter().any(|i| i.name.eq_ignore_ascii_case(n)))
                    .collect();
                v.sort();
                v
            })
            .unwrap_or_default();
        VoicePacks {
            selected: self.quick_voice(),
            installed,
            importable,
        }
    }

    /// Import one of the Luxify voices by name.
    pub fn voice_pack_import(&self, name: &str) -> Result<usize> {
        if name.is_empty() || name.contains(['/', '\\', '.']) {
            return Err(CoreError::Other("недопустимое имя набора".into()));
        }
        let dir = self
            .luxify_voices_dir()
            .ok_or_else(|| CoreError::Other("Luxify Assistant на этом ПК не найден".into()))?;
        let src = dir.join(name);
        self.quick_import_voices(&src, name)
    }
}
