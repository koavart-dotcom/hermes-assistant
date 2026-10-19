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
use ts_rs::TS;

use crate::chat::ChatEvent;
use crate::diag;
use crate::digest::{TelegramTarget, send_telegram, telegram_target};
use crate::fix;
use crate::inventory::{Access, Inventory};
use crate::supervisor::Supervisor;
use crate::wazuh::{self, Alert, Correlated, HostContext, Incident, WazuhConfig};

/// Result of [`Supervisor::wazuh_host_context`].
struct HostInfo {
    prompt: HostContext,
    summary: Option<String>,
}
use crate::{CoreError, Result};

/// One analysis turn may take this long before it is dropped.
const ANALYSIS_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct IngestOutcome {
    pub received: usize,
    /// Incident ids opened by this push.
    #[ts(type = "Array<number>")]
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

    /// (Re)start the Wazuh ingest listener from the saved config: stop the
    /// running one, start a new one when `enabled`. Server mode only.
    pub fn wazuh_listener_apply(self: &Arc<Self>) -> bool {
        let cfg = WazuhConfig::load(&self.ctx.layout);
        let mut slot = self
            .wazuh_listener
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(old) = slot.take() {
            old.cancel();
        }
        if !self.ctx.server || !cfg.enabled {
            log::info!("wazuh ingest is off");
            return false;
        }
        let cancel = CancellationToken::new();
        *slot = Some(cancel.clone());
        let sup = self.clone();
        tokio::spawn(async move {
            if let Err(e) = wazuh::serve_ingest(sup, cfg, cancel).await {
                log::error!("wazuh ingest stopped: {e}");
            }
        });
        true
    }

    pub fn wazuh_listener_running(&self) -> bool {
        self.wazuh_listener
            .lock()
            .map(|s| s.is_some())
            .unwrap_or(false)
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
        let cfg_diagnose = cfg.diagnose;
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
            let host = sup.wazuh_host_context(&inc, cfg_diagnose).await;
            if let Some(i) = sup.wazuh_state().get_mut(inc.id) {
                i.diag = host.summary.clone();
            }
            let result = sup.wazuh_analyze(&inc, &host.prompt).await;
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
                let mut inc = inc.clone();
                inc.diag = finished.as_ref().and_then(|i| i.diag.clone());
                if let Err(e) = send_telegram(tg, &wazuh::analysis_text(&inc, &text)).await {
                    log::warn!("wazuh: telegram: {e}");
                }
            }
        });
    }

    /// Plan a fix on an inventory host (nothing runs).
    pub fn fix_plan(
        &self,
        host: &str,
        action: &str,
        service: Option<&str>,
        incident_id: Option<u64>,
    ) -> Result<(crate::inventory::Host, fix::FixPlan)> {
        if let Some(id) = incident_id {
            let inc = self
                .wazuh_incident(id)
                .ok_or_else(|| CoreError::Other(format!("incident #{id} not found")))?;
            if inc.security {
                return Err(CoreError::Other(format!(
                    "incident #{id} is a security alert: the agent never fixes those, escalate to people"
                )));
            }
        }
        let inv = Inventory::load(&self.ctx.layout);
        let h = inv
            .find(host, host)
            .cloned()
            .ok_or_else(|| CoreError::Other(format!("{host} is not in the inventory")))?;
        let plan = fix::plan(&h, action, service)?;
        Ok((h, plan))
    }

    /// Run a confirmed fix: one at a time per host; result to Telegram when set up.
    pub async fn fix_run(
        &self,
        host: &str,
        action: &str,
        service: Option<&str>,
        incident_id: Option<u64>,
        by: &str,
    ) -> Result<fix::FixReport> {
        let (h, plan) = self.fix_plan(host, action, service, incident_id)?;
        {
            let mut busy = self.fix_busy.lock().unwrap_or_else(|e| e.into_inner());
            if !busy.insert(h.name.clone()) {
                return Err(CoreError::Other(format!(
                    "a fix is already running on {}",
                    h.name
                )));
            }
        }
        let report = fix::run(&self.ctx.layout, &h, plan, by).await;
        self.fix_busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&h.name);
        if let Some(tg) = self.wazuh_telegram(&WazuhConfig::load(&self.ctx.layout)) {
            let text = format!(
                "{} {} на {}{}{} · {} · {by}",
                if report.ok { "🔧" } else { "⚠️" },
                report.plan.title,
                report.plan.host,
                report
                    .plan
                    .service
                    .as_deref()
                    .map(|s| format!(" ({s})"))
                    .unwrap_or_default(),
                incident_id
                    .map(|i| format!(" · инцидент #{i}"))
                    .unwrap_or_default(),
                if report.ok {
                    "выполнено"
                } else {
                    "ошибка"
                },
            );
            if let Err(e) = send_telegram(&tg, &text).await {
                log::warn!("fix: telegram: {e}");
            }
        }
        Ok(report)
    }

    /// Inventory facts and, when allowed, the read-only probes of the alert's playbook.
    async fn wazuh_host_context(&self, inc: &Incident, diagnose: bool) -> HostInfo {
        let layout = &self.ctx.layout;
        let inv = Inventory::load(layout);
        let Some(host) = inv.find(&inc.agent_name, &inc.agent_ip).cloned() else {
            return HostInfo {
                prompt: HostContext::default(),
                summary: Some(format!("{} нет в инвентаре", inc.agent_name)),
            };
        };
        let about = [
            (!host.owner.is_empty()).then(|| format!("ответственный {}", host.owner)),
            (!host.criticality.is_empty()).then(|| format!("критичность {}", host.criticality)),
            (!host.notes.is_empty()).then(|| host.notes.clone()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("; ");
        let about = (!about.is_empty()).then_some(about);
        if !diagnose || host.access == Access::None {
            return HostInfo {
                prompt: HostContext { about, diag: None },
                summary: None,
            };
        }
        let ids = match &inc.sample {
            Some(a) => diag::playbook(host.os, a),
            None => Vec::new(),
        };
        let report = diag::run(layout, &host, &ids).await;
        HostInfo {
            summary: Some(report.summary()),
            prompt: HostContext {
                about,
                diag: Some(report.prompt_text()),
            },
        }
    }

    /// One Hermes turn in a new session; returns the final answer text.
    async fn wazuh_analyze(&self, inc: &Incident, host: &HostContext) -> Result<String> {
        if let Some(i) = self.wazuh_state().get_mut(inc.id) {
            i.analysis_state = "running".into();
        }
        let client = self.hermes_client()?;
        let title = format!("Wazuh #{} · {} · {}", inc.id, inc.agent_name, inc.rule_id);
        let session = client.create_session(Some(&title)).await?;
        let prompt = wazuh::analysis_prompt(inc, host);
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
