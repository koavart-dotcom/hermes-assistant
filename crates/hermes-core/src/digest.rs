//! Helpdesk digest: a short text built from the FreeScout snapshot for the
//! chat, the voice and Telegram. Sent by the shell itself (no model, no
//! tokens): Telegram through the Bot API with the token from Hermes' `.env`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::freescout::{Ticket, TicketsSnapshot};
use crate::{CoreError, Layout, Result};

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct DigestResult {
    /// Markdown for the chat / Telegram.
    pub text: String,
    /// One or two sentences for the voice.
    pub spoken: String,
    pub telegram_sent: bool,
    pub telegram_error: Option<String>,
}

fn line(t: &Ticket) -> String {
    let who = if t.customer.is_empty() {
        String::new()
    } else {
        format!(" — {}", t.customer)
    };
    let mark = match t.mark.as_str() {
        "new" => " 🆕",
        "updated" => " ✉️",
        _ => "",
    };
    format!("• #{} {}{}{}", t.number, t.subject, who, mark)
}

/// Markdown digest: summary, then up to `limit` tickets per section.
pub fn build_text(s: &TicketsSnapshot, limit: usize) -> String {
    let mut out = vec![format!("🧭 Хелпдеск: {}", s.summary)];
    if !s.mine.is_empty() {
        out.push(format!("\nНа вас ({}):", s.mine.len()));
        out.extend(s.mine.iter().take(limit).map(line));
        if s.mine.len() > limit {
            out.push(format!("… ещё {}", s.mine.len() - limit));
        }
    }
    if !s.unassigned.is_empty() {
        out.push(format!("\nБез исполнителя ({}):", s.unassigned.len()));
        out.extend(s.unassigned.iter().take(limit).map(line));
        if s.unassigned.len() > limit {
            out.push(format!("… ещё {}", s.unassigned.len() - limit));
        }
    }
    out.join("\n")
}

/// Spoken digest: the summary plus the newest subjects (max 3) so it stays short.
pub fn build_spoken(s: &TicketsSnapshot) -> String {
    let fresh: Vec<&Ticket> = s
        .mine
        .iter()
        .chain(s.unassigned.iter())
        .filter(|t| t.mark != "working")
        .take(3)
        .collect();
    if fresh.is_empty() {
        return s.summary.clone();
    }
    let subjects: Vec<String> = fresh
        .iter()
        .map(|t| t.subject.chars().take(60).collect::<String>())
        .collect();
    format!("{} Новое: {}.", s.summary, subjects.join("; "))
}

pub struct TelegramTarget {
    pub token: String,
    pub chat_id: String,
}

/// `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID` (or the first of `TELEGRAM_ALLOWED_USERS`).
pub fn telegram_target(layout: &Layout) -> Option<TelegramTarget> {
    let text = std::fs::read_to_string(layout.env_file()).ok()?;
    let kv: HashMap<String, String> = crate::envkeys::parse(&text).into_iter().collect();
    let token = kv.get("TELEGRAM_BOT_TOKEN")?.trim().to_string();
    let chat_id = kv
        .get("TELEGRAM_CHAT_ID")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            kv.get("TELEGRAM_ALLOWED_USERS")
                .and_then(|s| {
                    s.split([',', ';', ' '])
                        .map(str::trim)
                        .find(|x| !x.is_empty())
                })
                .map(str::to_string)
        })?;
    (!token.is_empty()).then_some(TelegramTarget { token, chat_id })
}

pub async fn send_telegram(target: &TelegramTarget, text: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("reqwest client");
    let url = format!("https://api.telegram.org/bot{}/sendMessage", target.token);
    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "chat_id": target.chat_id, "text": text, "disable_web_page_preview": true }))
        .send()
        .await
        .map_err(|e| CoreError::Other(format!("telegram: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CoreError::Other(format!(
            "telegram: HTTP {status} {}",
            body.chars().take(160).collect::<String>()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: u64, mark: &str, mine: bool) -> Ticket {
        Ticket {
            id: n,
            number: n,
            subject: format!("Тема {n}"),
            status: "active".into(),
            mailbox: "IT".into(),
            customer: if mine {
                "Иван".into()
            } else {
                String::new()
            },
            customer_email: String::new(),
            assignee: String::new(),
            tags: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            last_message: String::new(),
            url: String::new(),
            mark: mark.into(),
        }
    }

    #[test]
    fn text_and_spoken() {
        let s = TicketsSnapshot {
            fetched_at: 0,
            my_user_id: Some(4),
            mine: vec![t(1, "new", true)],
            unassigned: (2..8).map(|n| t(n, "working", false)).collect(),
            summary: "На вас 1 заявка, без исполнителя 6 заявок.".into(),
        };
        let text = build_text(&s, 3);
        assert!(text.starts_with("🧭 Хелпдеск: На вас 1 заявка"));
        assert!(text.contains("• #1 Тема 1 — Иван 🆕"));
        assert!(text.contains("… ещё 3"));
        assert_eq!(
            build_spoken(&s),
            "На вас 1 заявка, без исполнителя 6 заявок. Новое: Тема 1."
        );
        let quiet = TicketsSnapshot {
            mine: vec![],
            unassigned: vec![t(2, "working", false)],
            ..s
        };
        assert_eq!(build_spoken(&quiet), quiet.summary);
    }

    #[test]
    fn telegram_target_from_env() {
        let root = std::env::temp_dir().join(format!("hermes-digest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        std::fs::create_dir_all(&layout.hermes_home).unwrap();
        assert!(telegram_target(&layout).is_none());
        std::fs::write(
            layout.env_file(),
            "TELEGRAM_BOT_TOKEN=123:abc\nTELEGRAM_ALLOWED_USERS=42, 43\n",
        )
        .unwrap();
        let tg = telegram_target(&layout).unwrap();
        assert_eq!(tg.chat_id, "42");
        std::fs::write(
            layout.env_file(),
            "TELEGRAM_BOT_TOKEN=123:abc\nTELEGRAM_CHAT_ID=7\nTELEGRAM_ALLOWED_USERS=42\n",
        )
        .unwrap();
        assert_eq!(telegram_target(&layout).unwrap().chat_id, "7");
        let _ = std::fs::remove_dir_all(&root);
    }
}
