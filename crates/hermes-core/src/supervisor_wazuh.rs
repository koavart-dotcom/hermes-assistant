//! Wazuh flow on the supervisor (see `wazuh.rs`): store the alert, group it into
//! an incident, send the incident card to Telegram, and ask Hermes for an
//! analysis in its own session. Stage 1 — analysis only: every tool approval
//! Hermes asks for during that turn is denied.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::chat::ChatEvent;
use crate::digest::{TelegramTarget, send_telegram, telegram_target};
use crate::supervisor::Supervisor;
use crate::wazuh::{self, Alert, Correlated, Incident, WazuhConfig};
use crate::{CoreError, Result};

/// One analysis turn may take this long before it is dropped.
const ANALYSIS_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize)]
pub struct IngestOutcome {
    pub received: usize,
    /// Incident ids opened by this push.
    pub new_incidents: Vec<u64>,
    pub repeats: usize,
    /// Below `min_level` or an ignored rule.
    pub filtered: usize,
    pub invalid: usize,
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Supervisor {
    fn wazuh_state(&self) -> std::sync::MutexGuard<'_, wazuh::WazuhState> {
        self.wazuh.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn wazuh_incidents(&self) -> Vec<Incident> {
        self.wazuh_state().incidents.iter().cloned().collect()
    }

    pub fn wazuh_incident(&self, id: u64) -> Option<Incident> {
        self.wazuh_state()
            .incidents
            .iter()
            .find(|i| i.id == id)
            .cloned()
    }

    fn wazuh_telegram(&self, cfg: &WazuhConfig) -> Option<TelegramTarget> {
        if !cfg.telegram {
            return None;
        }
        let mut t = telegram_target(&self.ctx.layout)?;
        if !cfg.telegram_chat_id.trim().is_empty() {
            t.chat_id = cfg.telegram_chat_id.trim().to_string();
        }
        Some(t)
    }

    /// Take a push from the Wazuh integration (or `POST /control/v1/wazuh/alerts`).
    /// Returns at once; Telegram and the analysis run in the background.
    pub fn wazuh_ingest(self: &Arc<Self>, body: &Value) -> Result<IngestOutcome> {
        let layout = &self.ctx.layout;
        let cfg = WazuhConfig::load(layout);
        let items = wazuh::alerts_in(body);
        let mut out = IngestOutcome {
            received: items.len(),
            new_incidents: Vec::new(),
            repeats: 0,
            filtered: 0,
            invalid: 0,
        };
        let now = now_unix();
        for item in items {
            let Ok(alert) = Alert::from_value(item) else {
                out.invalid += 1;
                continue;
            };
            if let Err(e) = wazuh::append_jsonl(layout, "alerts.jsonl", item) {
                log::warn!("wazuh: store alert: {e}");
            }
            if !alert.wanted(&cfg) {
                out.filtered += 1;
                continue;
            }
            let correlated = self.wazuh_state().correlate(&alert, cfg.window_min, now);
            match correlated {
                Correlated::New(id) => {
                    out.new_incidents.push(id);
                    log::info!(
                        "wazuh: incident #{id} — {} rule {} level {}: {}",
                        alert.agent_name,
                        alert.rule_id,
                        alert.level,
                        alert.description
                    );
                    self.wazuh_on_new(id, &cfg, now);
                }
                Correlated::Repeat { id, count } => {
                    out.repeats += 1;
                    if wazuh::REPEAT_NOTICES.contains(&count) {
                        if let (Some(tg), Some(inc)) =
                            (self.wazuh_telegram(&cfg), self.wazuh_incident(id))
                        {
                            let text = wazuh::repeat_text(&inc);
                            tokio::spawn(async move {
                                if let Err(e) = send_telegram(&tg, &text).await {
                                    log::warn!("wazuh: telegram: {e}");
                                }
                            });
                        }
                    }
                }
            }
        }
        if out.received > 0 && out.invalid == out.received {
            return Err(CoreError::Other("no Wazuh alerts in the body".into()));
        }
        Ok(out)
    }

    fn wazuh_on_new(self: &Arc<Self>, id: u64, cfg: &WazuhConfig, now: u64) {
        let analyze = if !cfg.analyze {
            Some("off".to_string())
        } else if !self.ctx.layout.bootstrap_marker().exists() {
            Some("skipped: Hermes is not installed".to_string())
        } else if !self
            .wazuh_state()
            .take_budget(cfg.max_analyses_per_hour, now)
        {
            Some(format!(
                "skipped: limit {} analyses per hour",
                cfg.max_analyses_per_hour
            ))
        } else {
            None
        };
        if let Some(state) = &analyze {
            if let Some(inc) = self.wazuh_state().get_mut(id) {
                inc.analysis_state = state.clone();
            }
        }
        let Some(inc) = self.wazuh_incident(id) else {
            return;
        };
        let tg = self.wazuh_telegram(cfg);
        let sup = self.clone();
        tokio::spawn(async move {
            if let Some(tg) = &tg {
                let mut text = wazuh::card_text(&inc);
                text.push_str(match &analyze {
                    None => "\nРазбираю…",
                    Some(s) if s.starts_with("skipped: limit") => "\nРазбор пропущен: лимит в час.",
                    Some(_) => "",
                });
                if let Err(e) = send_telegram(tg, &text).await {
                    log::warn!("wazuh: telegram: {e}");
                }
            }
            if analyze.is_some() {
                return;
            }
            let result = sup.wazuh_analyze(&inc).await;
            let (state, text) = match result {
                Ok(t) => ("done", t),
                Err(e) => {
                    log::warn!("wazuh: analysis #{} failed: {e}", inc.id);
                    ("failed", format!("Разбор не получился: {e}"))
                }
            };
            let finished = {
                let mut st = sup.wazuh_state();
                st.get_mut(inc.id).map(|i| {
                    i.analysis_state = state.into();
                    i.analysis = Some(text.clone());
                    i.clone()
                })
            };
            if let Some(i) = &finished {
                let _ = wazuh::append_jsonl(
                    &sup.ctx.layout,
                    "incidents.jsonl",
                    &json!({ "incident": i, "analysis": text }),
                );
            }
            if let Some(tg) = &tg {
                if let Err(e) = send_telegram(tg, &wazuh::analysis_text(&inc, &text)).await {
                    log::warn!("wazuh: telegram: {e}");
                }
            }
        });
    }

    /// One Hermes turn in a new session; returns the final answer text.
    async fn wazuh_analyze(&self, inc: &Incident) -> Result<String> {
        if let Some(i) = self.wazuh_state().get_mut(inc.id) {
            i.analysis_state = "running".into();
        }
        let client = self.hermes_client()?;
        let title = format!("Wazuh #{} · {} · {}", inc.id, inc.agent_name, inc.rule_id);
        let session = client.create_session(Some(&title)).await?;
        let prompt = wazuh::analysis_prompt(inc);
        let (tx, mut rx) = mpsc::unbounded_channel::<ChatEvent>();
        let cancel = CancellationToken::new();
        let collect = async {
            let mut answer = String::new();
            let mut deltas = String::new();
            let mut error: Option<String> = None;
            while let Some(ev) = rx.recv().await {
                match ev {
                    ChatEvent::Delta { text, .. } => deltas.push_str(&text),
                    ChatEvent::Completed { content, .. } => answer = content,
                    ChatEvent::Failed { error: e, .. } => error = Some(e),
                    // Stage 1: no actions on machines; refuse whatever Hermes asks to run.
                    ChatEvent::ApprovalRequest {
                        run_id, command, ..
                    } => {
                        log::info!(
                            "wazuh: #{} denied a tool approval: {}",
                            inc.id,
                            command.unwrap_or_default()
                        );
                        if let Err(e) = client.approve(&run_id, "deny").await {
                            log::warn!("wazuh: deny approval: {e}");
                        }
                    }
                    _ => {}
                }
            }
            if answer.trim().is_empty() {
                answer = deltas;
            }
            match (answer.trim().is_empty(), error) {
                (true, Some(e)) => Err(CoreError::Other(e)),
                (true, None) => Err(CoreError::Other("empty answer".into())),
                (false, _) => Ok(answer),
            }
        };
        let run = async {
            let r = client.stream_chat(&session.id, &prompt, &tx, &cancel).await;
            drop(tx);
            r
        };
        let both = async { tokio::join!(run, collect) };
        match tokio::time::timeout(ANALYSIS_TIMEOUT, both).await {
            Ok((_, answer)) => answer,
            Err(_) => {
                cancel.cancel();
                Err(CoreError::Other(format!(
                    "no answer within {}s",
                    ANALYSIS_TIMEOUT.as_secs()
                )))
            }
        }
    }
}
