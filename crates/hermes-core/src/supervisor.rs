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
}

impl Supervisor {
    pub fn new(ctx: Arc<SetupContext>) -> Arc<Supervisor> {
        let (events, _) = broadcast::channel(1024);
        Arc::new(Supervisor {
            ctx,
            setup: Mutex::new(None),
            ollama_child: Mutex::new(None),
            events,
            history: Mutex::new(VecDeque::new()),
            seq: AtomicU64::new(0),
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

    pub async fn gateway_stop(&self) -> Result<()> {
        Gateway::new(self.ctx.layout.clone(), None)
            .stop(None, &CancellationToken::new())
            .await
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
