//! Chat side of the [`Supervisor`]: one streamed Hermes turn per session at a
//! time, a replayable stream of [`ChatEnvelope`]s for the webview and the
//! control API, stop and approval forwarding.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::chat::{ChatEvent, ChatMessage, HermesClient, SessionSummary};
use crate::gateway::Gateway;
use crate::supervisor::{HISTORY_LIMIT, Supervisor};
use crate::{CoreError, Result};

/// A chat event with its session and a monotonically increasing sequence.
#[derive(Debug, Clone, Serialize)]
pub struct ChatStamped {
    pub seq: u64,
    pub ts_unix: u64,
    pub session_id: String,
    pub event: ChatEvent,
}

/// A turn in flight: the Hermes run id (empty until `run.started`) and the
/// token that drops our SSE connection.
#[derive(Debug, Clone)]
pub struct ActiveTurn {
    pub run_id: String,
    pub cancel: CancellationToken,
}

impl Supervisor {
    /// Client for the local api_server with the key from the keyring.
    pub fn hermes_client(&self) -> Result<HermesClient> {
        let layout = &self.ctx.layout;
        let key = crate::secrets::api_server_key(layout)?;
        Ok(HermesClient::new(
            Gateway::new(layout.clone(), None).base_url(),
            key,
        ))
    }

    pub fn subscribe_chat(&self) -> broadcast::Receiver<ChatStamped> {
        self.chat_events.subscribe()
    }

    pub fn chat_last_seq(&self) -> u64 {
        self.chat_seq.load(Ordering::SeqCst)
    }

    pub fn chat_events_since(&self, after: u64, limit: usize) -> Vec<ChatStamped> {
        let h = self.chat_history.lock().unwrap_or_else(|e| e.into_inner());
        h.iter()
            .filter(|s| s.seq > after)
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn record_chat(&self, session_id: &str, event: ChatEvent) {
        let seq = self.chat_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let ts_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match &event {
            ChatEvent::Failed { error, .. } => {
                log::warn!(target: "chat", "session {session_id}: {error}")
            }
            ChatEvent::Delta { .. } | ChatEvent::Thinking { .. } => {}
            other => log::info!(
                target: "chat",
                "session {session_id}: {}",
                serde_json::to_string(other).unwrap_or_default()
            ),
        }
        let stamped = ChatStamped {
            seq,
            ts_unix,
            session_id: session_id.to_string(),
            event,
        };
        {
            let mut h = self.chat_history.lock().unwrap_or_else(|e| e.into_inner());
            if h.len() >= HISTORY_LIMIT {
                h.pop_front();
            }
            h.push_back(stamped.clone());
        }
        let _ = self.chat_events.send(stamped);
    }

    pub fn active_turn(&self, session_id: &str) -> Option<ActiveTurn> {
        self.chat_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .cloned()
    }

    pub async fn chat_sessions(&self) -> Result<Vec<SessionSummary>> {
        self.hermes_client()?.list_sessions(100).await
    }

    pub async fn chat_create_session(&self, title: Option<&str>) -> Result<SessionSummary> {
        self.hermes_client()?.create_session(title).await
    }

    pub async fn chat_delete_session(&self, session_id: &str) -> Result<()> {
        if let Some(turn) = self.active_turn(session_id) {
            turn.cancel.cancel();
        }
        self.hermes_client()?.delete_session(session_id).await
    }

    pub async fn chat_messages(&self, session_id: &str) -> Result<Vec<ChatMessage>> {
        self.hermes_client()?.messages(session_id, 500).await
    }

    /// Reserve the session for one turn and return the future that streams
    /// it; the caller spawns it. Events go to [`Self::record_chat`].
    pub fn chat_send(
        self: &Arc<Self>,
        session_id: &str,
        message: &str,
    ) -> Result<impl Future<Output = ()> + Send + 'static> {
        if message.trim().is_empty() {
            return Err(CoreError::Other("message is empty".into()));
        }
        let client = self.hermes_client()?;
        let cancel = {
            let mut runs = self.chat_runs.lock().unwrap_or_else(|e| e.into_inner());
            if runs.contains_key(session_id) {
                return Err(CoreError::Other(format!(
                    "session {session_id} already has a turn in progress"
                )));
            }
            let c = CancellationToken::new();
            runs.insert(
                session_id.to_string(),
                ActiveTurn {
                    run_id: String::new(),
                    cancel: c.clone(),
                },
            );
            c
        };
        let sup = self.clone();
        let session_id = session_id.to_string();
        let message = message.to_string();
        Ok(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<ChatEvent>();
            let pump = {
                let sup = sup.clone();
                let session_id = session_id.clone();
                async move {
                    while let Some(ev) = rx.recv().await {
                        if let ChatEvent::RunStarted { run_id } = &ev {
                            let mut runs = sup.chat_runs.lock().unwrap_or_else(|e| e.into_inner());
                            if let Some(t) = runs.get_mut(&session_id) {
                                t.run_id = run_id.clone();
                            }
                        }
                        sup.record_chat(&session_id, ev);
                    }
                }
            };
            let run = {
                let session_id = session_id.clone();
                async move {
                    let _ = client
                        .stream_chat(&session_id, &message, &tx, &cancel)
                        .await;
                    drop(tx);
                }
            };
            tokio::join!(run, pump);
            let mut runs = sup.chat_runs.lock().unwrap_or_else(|e| e.into_inner());
            runs.remove(&session_id);
        })
    }

    /// Interrupt the turn of a session: ask Hermes to stop the run when we
    /// know its id, otherwise just drop the stream.
    pub async fn chat_stop(&self, session_id: &str) -> Result<()> {
        let Some(turn) = self.active_turn(session_id) else {
            return Ok(());
        };
        if turn.run_id.is_empty() {
            turn.cancel.cancel();
            return Ok(());
        }
        self.hermes_client()?.stop(&turn.run_id).await
    }

    pub async fn chat_approve(&self, run_id: &str, choice: &str) -> Result<()> {
        self.hermes_client()?.approve(run_id, choice).await
    }
}

/// Shared state type for the chat side (constructed in `Supervisor::new`).
pub type ChatRuns = std::collections::HashMap<String, ActiveTurn>;
pub type ChatHistory = VecDeque<ChatStamped>;
