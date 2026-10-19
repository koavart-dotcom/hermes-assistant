//! Process-wide supervisor shared by the Tauri commands and the control API:
//! at most one running setup, the Ollama child we own, and a replayable
//! stream of setup events (also mirrored into the application log).

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::cli::{Output, command_for, hermes_env, run_streaming};
use crate::events::SetupEvent;
use crate::gateway::Gateway;
use crate::ollama::Ollama;
use crate::setup::{self, SetupContext, SetupOptions, SetupPlan};
use crate::status::{self, StatusSnapshot};
use crate::supervisor_chat::{ChatHistory, ChatRuns, ChatStamped};
use crate::{CoreError, Result};

/// How many events are kept for replay (`events_since`).
pub const HISTORY_LIMIT: usize = 2000;

/// A setup event with a monotonically increasing sequence number.
#[derive(Debug, Clone, Serialize)]
pub struct Stamped {
    pub seq: u64,
    pub ts_unix: u64,
    pub event: SetupEvent,
}

pub struct Supervisor {
    pub ctx: Arc<SetupContext>,
    setup: Mutex<Option<CancellationToken>>,
    ollama_child: Mutex<Option<tokio::process::Child>>,
    events: broadcast::Sender<Stamped>,
    history: Mutex<VecDeque<Stamped>>,
    seq: AtomicU64,
    // Chat side (see supervisor_chat.rs).
    pub(crate) chat_events: broadcast::Sender<ChatStamped>,
    pub(crate) chat_history: Mutex<ChatHistory>,
    pub(crate) chat_seq: AtomicU64,
    pub(crate) chat_runs: Mutex<ChatRuns>,
    // Voice side (see supervisor_voice.rs): the Hermes web server we spawn.
    pub(crate) dashboard_child: Mutex<Option<tokio::process::Child>>,
    pub(crate) voice_setup: Mutex<Option<CancellationToken>>,
    /// Last search query of a `web.*` quick command (see supervisor_quick.rs).
    pub(crate) last_query: Mutex<Option<String>>,
    /// Conversation id for which a reply draft was requested (see supervisor_quick.rs).
    pub(crate) pending_draft: Mutex<Option<u64>>,
    /// Wazuh incidents and the analysis budget (see supervisor_wazuh.rs).
    pub(crate) wazuh: Mutex<crate::wazuh::WazuhState>,
    /// The running Wazuh ingest listener (server mode), see supervisor_wazuh.rs.
    pub(crate) wazuh_listener: Mutex<Option<CancellationToken>>,
    /// Hosts with a fix in progress (one at a time per host).
    pub(crate) fix_busy: Mutex<std::collections::HashSet<String>>,
    /// HTTPS access from the network (see supervisor_network.rs).
    pub(crate) network_app: Mutex<Option<axum::Router>>,
    pub(crate) network_listener: Mutex<Option<CancellationToken>>,
    pub(crate) network_state: Mutex<crate::netaccess::NetworkStatus>,
    /// Self-update of the service (see server_update.rs).
    pub(crate) update_state: Mutex<crate::server_update::UpdateStatus>,
    /// Cancelled when hermesd should exit so the SCM restarts it (after an update).
    pub restart_requested: CancellationToken,
}

impl Supervisor {
    pub fn new(ctx: Arc<SetupContext>) -> Arc<Supervisor> {
        let (events, _) = broadcast::channel(1024);
        let (chat_events, _) = broadcast::channel(4096);
        Arc::new(Supervisor {
            ctx,
            setup: Mutex::new(None),
            ollama_child: Mutex::new(None),
            events,
            history: Mutex::new(VecDeque::new()),
            seq: AtomicU64::new(0),
            chat_events,
            chat_history: Mutex::new(VecDeque::new()),
            chat_seq: AtomicU64::new(0),
            chat_runs: Mutex::new(ChatRuns::new()),
            dashboard_child: Mutex::new(None),
            voice_setup: Mutex::new(None),
            last_query: Mutex::new(None),
            pending_draft: Mutex::new(None),
            wazuh: Mutex::new(crate::wazuh::WazuhState::default()),
            wazuh_listener: Mutex::new(None),
            fix_busy: Mutex::new(std::collections::HashSet::new()),
            network_app: Mutex::new(None),
            network_listener: Mutex::new(None),
            network_state: Mutex::new(crate::netaccess::NetworkStatus::default()),
            update_state: Mutex::new(crate::server_update::UpdateStatus::default()),
            restart_requested: CancellationToken::new(),
        })
    }

    pub fn plan(&self) -> SetupPlan {
        self.ctx.plan()
    }

    pub fn setup_running(&self) -> bool {
        self.setup
            .lock()
            .map(|s| s.is_some())
            .unwrap_or_else(|e| e.into_inner().is_some())
    }

    /// Live events; receivers that fall behind get `Lagged` and continue.
    pub fn subscribe(&self) -> broadcast::Receiver<Stamped> {
        self.events.subscribe()
    }

    pub fn last_seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// Events with `seq > after`, oldest first, at most `limit`.
    pub fn events_since(&self, after: u64, limit: usize) -> Vec<Stamped> {
        let h = self.history.lock().unwrap_or_else(|e| e.into_inner());
        h.iter()
            .filter(|s| s.seq > after)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Record an event: log file, replay buffer, live subscribers.
    pub fn record(&self, event: SetupEvent) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let ts_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match &event {
            SetupEvent::Log { line } => log::info!(target: "setup", "{line}"),
            SetupEvent::Failed { message } => log::error!(target: "setup", "{message}"),
            other => log::info!(
                target: "setup",
                "{}",
                serde_json::to_string(other).unwrap_or_default()
            ),
        }
        let stamped = Stamped {
            seq,
            ts_unix,
            event,
        };
        {
            let mut h = self.history.lock().unwrap_or_else(|e| e.into_inner());
            if h.len() >= HISTORY_LIMIT {
                h.pop_front();
            }
            h.push_back(stamped.clone());
        }
        let _ = self.events.send(stamped);
    }

    /// Reserve the setup slot and return the future that runs the pipeline.
    /// The caller spawns it on its runtime (Tauri or the control server);
    /// the slot is released when the future completes.
    pub fn start_setup(
        self: &Arc<Self>,
        opts: SetupOptions,
    ) -> Result<impl Future<Output = ()> + Send + 'static> {
        let cancel = {
            let mut slot = self.setup.lock().unwrap_or_else(|e| e.into_inner());
            if slot.is_some() {
                return Err(CoreError::Other("setup is already running".into()));
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
                    let result = setup::run(&sup.ctx, &opts, &tx, &cancel).await;
                    drop(tx);
                    if let Ok(Some(child)) = result {
                        sup.adopt_ollama(child);
                    }
                }
            };
            tokio::join!(run, pump);
            let mut slot = sup.setup.lock().unwrap_or_else(|e| e.into_inner());
            *slot = None;
        })
    }

    pub fn cancel_setup(&self) {
        let slot = self.setup.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = slot.as_ref() {
            c.cancel();
        }
    }

    fn adopt_ollama(&self, child: tokio::process::Child) {
        let mut c = self.ollama_child.lock().unwrap_or_else(|e| e.into_inner());
        *c = Some(child);
    }

    fn gateway(&self) -> Gateway {
        let layout = self.ctx.layout.clone();
        let key = crate::secrets::api_server_key(&layout).ok();
        Gateway::new(layout, key)
    }

    pub async fn status(&self) -> Result<StatusSnapshot> {
        let layout = &self.ctx.layout;
        let ollama = Ollama::new(layout.clone());
        let gateway = self.gateway();
        status::snapshot(layout, &self.ctx.pin.version, &ollama, &gateway).await
    }

    /// The gateway and its api_server answer `/health`.
    pub async fn gateway_healthy(&self) -> bool {
        self.gateway().is_healthy().await
    }

    /// Start Ollama (if installed) and the Hermes gateway.
    pub async fn gateway_start(&self) -> Result<()> {
        let ollama = Ollama::new(self.ctx.layout.clone());
        if ollama.is_installed() {
            if let Ok(Some(child)) = ollama.start().await {
                self.adopt_ollama(child);
            }
        }
        self.gateway().start(None, &CancellationToken::new()).await
    }

    /// At app start: restart our Ollama sidecar so it runs with the current
    /// environment (context length, port), then make sure it is up. A
    /// sidecar left over from a previous app version keeps the old settings
    /// otherwise. No-op when Ollama is not installed yet.
    pub async fn ollama_refresh(&self) -> Result<()> {
        let ollama = Ollama::new(self.ctx.layout.clone());
        if !ollama.is_installed() {
            return Ok(());
        }
        if ollama.is_running().await {
            log::info!("restarting the Ollama sidecar with the current environment");
            ollama.stop_owned().await?;
        }
        if let Some(child) = ollama.start().await? {
            self.adopt_ollama(child);
        }
        Ok(())
    }

    /// At app start: make sure the bundled permission plugin is installed
    /// and enabled; when its files changed and the gateway is running,
    /// restart the gateway so it loads the new version. No-op before the
    /// first setup (Hermes not installed).
    pub async fn plugins_ensure(&self) -> Result<()> {
        let layout = &self.ctx.layout;
        if !layout.bootstrap_marker().exists() {
            return Ok(());
        }
        let cancel = CancellationToken::new();
        let changed =
            crate::plugins::install_plugins(layout, &self.ctx.plugins_dir, None, &cancel).await?;
        if changed {
            let gateway = self.gateway();
            if gateway.is_healthy().await {
                log::info!("Hermes plugins updated; restarting the Hermes gateway");
                gateway.stop(None, &cancel).await?;
                gateway.start(None, &cancel).await?;
            }
        }
        Ok(())
    }

    /// Copy bundled skills into `$HERMES_HOME/skills` (Hermes picks them up per
    /// session, no restart needed).
    pub fn skills_ensure(&self) -> Result<Vec<String>> {
        let layout = &self.ctx.layout;
        if !layout.bootstrap_marker().exists() {
            return Ok(Vec::new());
        }
        let mut updated =
            crate::plugins::sync_all_skills(&self.ctx.skills_dir, &layout.hermes_home)?;
        // Persona ships next to the skills: resources/hermes-skills/../hermes-persona/SOUL.md
        if let Some(res) = self.ctx.skills_dir.parent() {
            let soul = res.join("hermes-persona").join("SOUL.md");
            if soul.is_file() && crate::plugins::sync_soul(&soul, &layout.hermes_home)? {
                updated.push("SOUL.md".into());
            }
        }
        if !updated.is_empty() {
            log::info!("skills updated: {}", updated.join(", "));
        }
        Ok(updated)
    }

    pub async fn gateway_stop(&self) -> Result<()> {
        Gateway::new(self.ctx.layout.clone(), None)
            .stop(None, &CancellationToken::new())
            .await
    }

    /// Stop then start the gateway (picks up `.env` / config changes).
    pub async fn gateway_restart(&self) -> Result<()> {
        if let Err(e) = self.gateway_stop().await {
            log::warn!("gateway stop before restart: {e}");
        }
        self.gateway_start().await
    }

    /// Run `hermes <args>` and return its output even when it exits non-zero
    /// (the caller decides what a failure means).
    pub async fn hermes_cli(&self, args: &[String], timeout: Duration) -> Result<Output> {
        let layout = &self.ctx.layout;
        let cli = layout.hermes_cli();
        if !cli.exists() {
            return Err(CoreError::Other(format!(
                "Hermes CLI not found at {}",
                cli.display()
            )));
        }
        let mut cmd = command_for(&cli);
        cmd.args(args);
        hermes_env(&mut cmd, layout);
        let cancel = CancellationToken::new();
        match tokio::time::timeout(timeout, run_streaming(cmd, "hermes", None, &cancel)).await {
            Ok(r) => r,
            Err(_) => {
                cancel.cancel();
                Err(CoreError::Other(format!(
                    "hermes {} timed out after {}s",
                    args.join(" "),
                    timeout.as_secs()
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Layout;
    use crate::bootstrap::Pin;
    use std::path::{Path, PathBuf};

    fn sup() -> Arc<Supervisor> {
        Supervisor::new(Arc::new(SetupContext {
            layout: Layout::from_roots(Path::new("/lad"), Path::new("/data")),
            script: PathBuf::from("install.ps1"),
            plugins_dir: PathBuf::from("plugins"),
            quick_pack: PathBuf::from("quick.json"),
            skills_dir: PathBuf::from("skills"),
            server: false,
            pin: Pin {
                version: "v0".into(),
                commit: "abc".into(),
                branch: "main".into(),
            },
        }))
    }

    #[test]
    fn records_number_and_replay_events() {
        let s = sup();
        let mut rx = s.subscribe();
        s.record(SetupEvent::Done);
        s.record(SetupEvent::Log {
            line: "hello".into(),
        });
        assert_eq!(s.last_seq(), 2);
        let all = s.events_since(0, 10);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].seq, 1);
        assert_eq!(s.events_since(1, 10).len(), 1);
        assert_eq!(s.events_since(0, 1).len(), 1);
        assert_eq!(rx.try_recv().unwrap().seq, 1);
    }

    #[test]
    fn history_is_bounded() {
        let s = sup();
        for _ in 0..(HISTORY_LIMIT + 5) {
            s.record(SetupEvent::Done);
        }
        let all = s.events_since(0, usize::MAX);
        assert_eq!(all.len(), HISTORY_LIMIT);
        assert_eq!(all[0].seq, 6);
    }

    #[tokio::test]
    async fn setup_slot_is_exclusive_and_released() {
        let s = sup();
        let opts = SetupOptions {
            model: "qwen3.5".into(),
            consent: true,
            autostart: false,
        };
        assert!(!s.setup_running());
        let fut = s.start_setup(opts.clone()).unwrap();
        assert!(s.setup_running());
        assert!(s.start_setup(opts.clone()).is_err());
        // On this (non-Windows) box the pipeline fails immediately; the slot
        // must still be released and a Failed event recorded.
        fut.await;
        assert!(!s.setup_running());
        assert!(
            s.events_since(0, 100)
                .iter()
                .any(|e| matches!(e.event, SetupEvent::Failed { .. }))
        );
        assert!(s.start_setup(opts).is_ok());
    }
}
