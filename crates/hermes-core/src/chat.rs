//! Chat with Hermes Agent through its api_server **Sessions API** (v0.22.2):
//! sessions with history on the Hermes side, one streamed turn per message
//! (`POST /api/sessions/{id}/chat/stream`, SSE), approvals and stop through
//! `/v1/runs/{run_id}/...`. Verified against `gateway/platforms/api_server.py`
//! at the pinned commit — re-check on every Hermes bump.
//!
//! SSE wire format: `event: <name>\ndata: <json>\n\n`, comment frames
//! (`: keepalive`) in between. Event names we consume: `run.started`,
//! `assistant.delta`, `tool.progress` (thinking), `tool.started`,
//! `tool.completed`, `tool.failed`, `assistant.commentary`, `approval.request`,
//! `approval.responded`, `assistant.completed`, `run.failed`, `run.cancelled`.

use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::{CoreError, Result};

// One Hermes session as listed by `GET /api/sessions`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub message_count: u32,
    pub started_at: Option<f64>,
    pub last_active: Option<f64>,
    pub model: Option<String>,
}

// One stored message of a session (`GET /api/sessions/{id}/messages`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ChatMessage {
    pub id: Option<String>,
    pub role: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub timestamp: Option<f64>,
}

// Typed view of the session stream, as the UI consumes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum ChatEvent {
    RunStarted {
        run_id: String,
    },
    Delta {
        run_id: String,
        text: String,
    },
    Thinking {
        run_id: String,
        text: String,
    },
    ToolStarted {
        run_id: String,
        tool: String,
        preview: Option<String>,
    },
    ToolFinished {
        run_id: String,
        tool: String,
        ok: bool,
        preview: Option<String>,
    },
    Commentary {
        run_id: String,
        text: String,
    },
    /// Hermes waits for `POST /v1/runs/{run_id}/approval` with one of `choices`.
    ApprovalRequest {
        run_id: String,
        command: Option<String>,
        description: Option<String>,
        choices: Vec<String>,
    },
    ApprovalResponded {
        run_id: String,
        choice: Option<String>,
    },
    Completed {
        run_id: String,
        content: String,
    },
    Failed {
        run_id: Option<String>,
        error: String,
    },
    Cancelled {
        run_id: String,
    },
    /// The stream closed; always the last event of a turn.
    Ended {
        run_id: Option<String>,
    },
}

// A chat event tagged with the session it belongs to (what the UI receives).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ChatEnvelope {
    pub session_id: String,
    pub event: ChatEvent,
}

pub type ChatSink = mpsc::UnboundedSender<ChatEvent>;

// --------------------------------------------------------------- parsing --

/// Split one SSE frame into `(event name, data)`; `None` for comment or
/// empty frames. Multi-line `data:` is joined with `\n` per the SSE spec.
pub fn parse_frame(frame: &str) -> Option<(Option<String>, String)> {
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in frame.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            event = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("data:") {
            data.push(v.strip_prefix(' ').unwrap_or(v));
        }
        // `id:` and `: comment` lines are ignored.
    }
    if data.is_empty() {
        None
    } else {
        Some((event, data.join("\n")))
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn num_field(v: &Value, key: &str) -> Option<f64> {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Text of a message `content` that may be a string or a multimodal list.
pub fn content_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| match p {
                Value::String(s) => Some(s.clone()),
                Value::Object(_) => p.get("text").and_then(Value::as_str).map(str::to_string),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub fn session_from_value(v: &Value) -> Option<SessionSummary> {
    Some(SessionSummary {
        id: str_field(v, "id")?,
        title: str_field(v, "title").filter(|t| !t.trim().is_empty()),
        preview: str_field(v, "preview"),
        message_count: v
            .get("message_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32,
        started_at: num_field(v, "started_at"),
        last_active: num_field(v, "last_active"),
        model: str_field(v, "model"),
    })
}

pub fn message_from_value(v: &Value) -> Option<ChatMessage> {
    Some(ChatMessage {
        id: str_field(v, "id"),
        role: str_field(v, "role")?,
        content: content_text(v.get("content")),
        tool_name: str_field(v, "tool_name"),
        timestamp: num_field(v, "timestamp"),
    })
}

/// Map a raw stream event to [`ChatEvent`]; `None` for events the UI does
/// not need (`message.started`, `run.completed`, keepalives, unknown names).
pub fn map_event(name: &str, data: &Value) -> Option<ChatEvent> {
    let run_id = str_field(data, "run_id").unwrap_or_default();
    let ev = match name {
        "run.started" => ChatEvent::RunStarted { run_id },
        "assistant.delta" => ChatEvent::Delta {
            run_id,
            text: str_field(data, "delta").unwrap_or_default(),
        },
        "tool.progress" => ChatEvent::Thinking {
            run_id,
            text: str_field(data, "delta").unwrap_or_default(),
        },
        "tool.started" => ChatEvent::ToolStarted {
            run_id,
            tool: str_field(data, "tool_name").unwrap_or_else(|| "tool".into()),
            preview: str_field(data, "preview"),
        },
        "tool.completed" | "tool.failed" => ChatEvent::ToolFinished {
            run_id,
            tool: str_field(data, "tool_name").unwrap_or_else(|| "tool".into()),
            ok: name == "tool.completed",
            preview: str_field(data, "preview"),
        },
        "assistant.commentary" => ChatEvent::Commentary {
            run_id,
            text: str_field(data, "text").unwrap_or_default(),
        },
        "approval.request" => ChatEvent::ApprovalRequest {
            run_id,
            command: str_field(data, "command"),
            description: str_field(data, "description"),
            choices: data
                .get("choices")
                .and_then(Value::as_array)
                .map(|c| {
                    c.iter()
                        .filter_map(|x| match x {
                            Value::String(s) => Some(s.clone()),
                            Value::Object(_) => {
                                str_field(x, "choice").or_else(|| str_field(x, "id"))
                            }
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_else(|| vec!["once".into(), "deny".into()]),
        },
        "approval.responded" => ChatEvent::ApprovalResponded {
            run_id,
            choice: str_field(data, "choice"),
        },
        "assistant.completed" => ChatEvent::Completed {
            run_id,
            content: content_text(data.get("content")),
        },
        "run.failed" | "run.interrupted" => ChatEvent::Failed {
            run_id: Some(run_id),
            error: str_field(data, "error").unwrap_or_else(|| "run failed".into()),
        },
        "run.cancelled" => ChatEvent::Cancelled { run_id },
        _ => return None,
    };
    Some(ev)
}

// ---------------------------------------------------------------- client --

#[derive(Debug, Clone)]
pub struct HermesClient {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

pub const APPROVAL_CHOICES: [&str; 4] = ["once", "session", "always", "deny"];

impl HermesClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> HermesClient {
        HermesClient {
            base_url: base_url.into(),
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn check(resp: reqwest::Response, what: &str) -> Result<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        Err(CoreError::Other(format!(
            "hermes api {what}: HTTP {status} {}",
            body.chars().take(400).collect::<String>()
        )))
    }

    pub async fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>> {
        let resp = self
            .client
            .get(self.url(&format!("/api/sessions?limit={limit}&offset=0")))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let v: Value = Self::check(resp, "list sessions").await?.json().await?;
        Ok(v.get("data")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(session_from_value).collect())
            .unwrap_or_default())
    }

    pub async fn create_session(&self, title: Option<&str>) -> Result<SessionSummary> {
        let mut body = json!({});
        if let Some(t) = title {
            body["title"] = json!(t);
        }
        let resp = self
            .client
            .post(self.url("/api/sessions"))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(15))
            .json(&body)
            .send()
            .await?;
        let v: Value = Self::check(resp, "create session").await?.json().await?;
        v.get("session")
            .and_then(session_from_value)
            .ok_or_else(|| CoreError::Protocol("create session: no `session` in response".into()))
    }

    pub async fn delete_session(&self, id: &str) -> Result<()> {
        let resp = self
            .client
            .delete(self.url(&format!("/api/sessions/{id}")))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        Self::check(resp, "delete session").await?;
        Ok(())
    }

    pub async fn rename_session(&self, id: &str, title: &str) -> Result<()> {
        let resp = self
            .client
            .patch(self.url(&format!("/api/sessions/{id}")))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(15))
            .json(&json!({ "title": title }))
            .send()
            .await?;
        Self::check(resp, "rename session").await?;
        Ok(())
    }

    /// Stored messages, oldest first.
    pub async fn messages(&self, id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let resp = self
            .client
            .get(self.url(&format!(
                "/api/sessions/{id}/messages?limit={limit}&order=oldest"
            )))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        let v: Value = Self::check(resp, "session messages").await?.json().await?;
        Ok(v.get("data")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(message_from_value).collect())
            .unwrap_or_default())
    }

    /// Send one user message and stream the turn into `sink`. Returns when
    /// the stream closes; the last event sent is always [`ChatEvent::Ended`].
    /// Cancelling only drops the connection — call [`stop`](Self::stop) to
    /// interrupt the agent.
    pub async fn stream_chat(
        &self,
        session_id: &str,
        message: &str,
        sink: &ChatSink,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let result = self
            .stream_chat_inner(session_id, message, sink, cancel)
            .await;
        let run_id = match &result {
            Ok(r) => r.clone(),
            Err(_) => None,
        };
        if let Err(e) = &result {
            let _ = sink.send(ChatEvent::Failed {
                run_id: None,
                error: e.to_string(),
            });
        }
        let _ = sink.send(ChatEvent::Ended { run_id });
        result.map(|_| ())
    }

    async fn stream_chat_inner(
        &self,
        session_id: &str,
        message: &str,
        sink: &ChatSink,
        cancel: &CancellationToken,
    ) -> Result<Option<String>> {
        let resp = self
            .client
            .post(self.url(&format!("/api/sessions/{session_id}/chat/stream")))
            .bearer_auth(&self.api_key)
            .header("Accept", "text/event-stream")
            .json(&json!({ "message": message }))
            .send()
            .await?;
        let resp = Self::check(resp, "chat stream").await?;
        let mut stream = resp.bytes_stream();
        let mut buf: Vec<u8> = Vec::new();
        let mut run_id: Option<String> = None;
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(CoreError::Cancelled),
                c = stream.next() => c,
            };
            let Some(chunk) = chunk else { break };
            buf.extend_from_slice(&chunk?);
            while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
                let frame: Vec<u8> = buf.drain(..pos + 2).collect();
                let text = String::from_utf8_lossy(&frame).replace("\r\n", "\n");
                let Some((name, data)) = parse_frame(&text) else {
                    continue;
                };
                let Ok(value) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                let name = name
                    .or_else(|| str_field(&value, "event"))
                    .unwrap_or_default();
                if let Some(ev) = map_event(&name, &value) {
                    if let ChatEvent::RunStarted { run_id: id } = &ev {
                        run_id = Some(id.clone());
                    }
                    let _ = sink.send(ev);
                }
            }
        }
        Ok(run_id)
    }

    /// Resolve a pending approval: `once`, `session`, `always` or `deny`.
    pub async fn approve(&self, run_id: &str, choice: &str) -> Result<()> {
        if !APPROVAL_CHOICES.contains(&choice) {
            return Err(CoreError::Other(format!(
                "invalid approval choice `{choice}`"
            )));
        }
        let resp = self
            .client
            .post(self.url(&format!("/v1/runs/{run_id}/approval")))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(15))
            .json(&json!({ "choice": choice }))
            .send()
            .await?;
        Self::check(resp, "approval").await?;
        Ok(())
    }

    /// Interrupt a running turn.
    pub async fn stop(&self, run_id: &str) -> Result<()> {
        let resp = self
            .client
            .post(self.url(&format!("/v1/runs/{run_id}/stop")))
            .bearer_auth(&self.api_key)
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        Self::check(resp, "stop").await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_frames() {
        assert_eq!(
            parse_frame("event: assistant.delta\ndata: {\"delta\":\"hi\"}\n\n"),
            Some((Some("assistant.delta".into()), "{\"delta\":\"hi\"}".into()))
        );
        assert_eq!(parse_frame(": keepalive\n\n"), None);
        assert_eq!(
            parse_frame("id: 3\ndata: a\ndata: b\n\n"),
            Some((None, "a\nb".into()))
        );
    }

    #[test]
    fn maps_stream_events() {
        let d = json!({"run_id": "run_1", "delta": "Привет"});
        assert_eq!(
            map_event("assistant.delta", &d),
            Some(ChatEvent::Delta {
                run_id: "run_1".into(),
                text: "Привет".into()
            })
        );
        let a = json!({"run_id": "run_1", "command": "rm -rf x", "description": "Deletes files",
                       "choices": ["once", "session", "always", "deny"]});
        match map_event("approval.request", &a) {
            Some(ChatEvent::ApprovalRequest {
                command, choices, ..
            }) => {
                assert_eq!(command.as_deref(), Some("rm -rf x"));
                assert_eq!(choices.len(), 4);
            }
            other => panic!("unexpected {other:?}"),
        }
        let c = json!({"run_id": "run_1", "content": [{"type": "text", "text": "done"}]});
        assert_eq!(
            map_event("assistant.completed", &c),
            Some(ChatEvent::Completed {
                run_id: "run_1".into(),
                content: "done".into()
            })
        );
        assert_eq!(map_event("message.started", &json!({})), None);
        assert!(matches!(
            map_event(
                "tool.failed",
                &json!({"run_id": "r", "tool_name": "terminal"})
            ),
            Some(ChatEvent::ToolFinished { ok: false, .. })
        ));
    }

    #[test]
    fn converts_session_and_message_rows() {
        let s = session_from_value(&json!({
            "id": "api_1", "title": "  ", "message_count": 3, "started_at": "1760600000.5",
            "last_active": 1760600100, "preview": "hi"
        }))
        .unwrap();
        assert_eq!(s.title, None);
        assert_eq!(s.message_count, 3);
        assert_eq!(s.started_at, Some(1760600000.5));
        assert_eq!(s.last_active, Some(1760600100.0));
        let m =
            message_from_value(&json!({"role": "user", "content": "x", "timestamp": 1.0})).unwrap();
        assert_eq!(m.content, "x");
        assert!(message_from_value(&json!({"content": "no role"})).is_none());
    }

    #[test]
    fn rejects_unknown_choice() {
        let c = HermesClient::new("http://127.0.0.1:1", "k");
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(rt.block_on(c.approve("run_x", "maybe")).is_err());
    }
}
