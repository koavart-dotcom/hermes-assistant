//! Morning briefing (the «digest»): calendar, mail, Jira and the helpdesk in
//! one short text for the chat, the voice and Telegram. Built by the shell
//! itself (no model, no tokens); every source is optional — one that is not
//! configured is skipped, one that fails shows a warning line and the rest
//! still go out. Telegram through the Bot API with the token from Hermes' `.env`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::calendar::CalendarEvent;
use crate::freescout::{Ticket, TicketsSnapshot};
use crate::jira::JiraIssue;
use crate::mail::MailSummary;
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

/// What each source returned; `None` — not configured, `Err` — the error text.
#[derive(Default)]
pub struct Briefing {
    pub events: Option<std::result::Result<Vec<CalendarEvent>, String>>,
    pub mail: Option<std::result::Result<MailSummary, String>>,
    pub jira: Option<std::result::Result<Vec<JiraIssue>, String>>,
    pub tickets: Option<std::result::Result<TicketsSnapshot, String>>,
}

/// «1 встреча», «3 встречи», «5 встреч».
pub fn plural(n: usize, one: &str, few: &str, many: &str) -> String {
    let w = match (n % 10, n % 100) {
        (1, x) if x != 11 => one,
        (2..=4, x) if !(12..=14).contains(&x) => few,
        _ => many,
    };
    format!("{n} {w}")
}

pub fn briefing_text(b: &Briefing, date: &str) -> String {
    let mut out = vec![format!("☀️ Сводка на {date}")];
    match &b.events {
        Some(Ok(ev)) if ev.is_empty() => out.push("\n📅 Встреч сегодня нет.".into()),
        Some(Ok(ev)) => {
            out.push(format!("\n📅 Календарь ({}):", ev.len()));
            out.extend(ev.iter().map(crate::calendar::line));
        }
        Some(Err(e)) => out.push(format!("\n⚠️ Календарь: {e}")),
        None => {}
    }
    match &b.mail {
        Some(Ok(m)) => {
            out.push(format!(
                "\n✉️ Почта: непрочитанных {}, важных {}",
                m.unread, m.important_unread
            ));
            out.extend(
                m.important
                    .iter()
                    .map(|i| format!("• {} — {}", i.from, i.subject)),
            );
        }
        Some(Err(e)) => out.push(format!("\n⚠️ Почта: {e}")),
        None => {}
    }
    match &b.jira {
        Some(Ok(j)) if j.is_empty() => out.push("\n📋 Jira: открытых задач нет.".into()),
        Some(Ok(j)) => {
            out.push(format!("\n📋 Jira ({}):", j.len()));
            out.extend(j.iter().take(8).map(crate::jira::line));
            if j.len() > 8 {
                out.push(format!("… ещё {}", j.len() - 8));
            }
        }
        Some(Err(e)) => out.push(format!("\n⚠️ {e}")),
        None => {}
    }
    match &b.tickets {
        Some(Ok(t)) => out.push(format!("\n{}", build_text(t, 8))),
        Some(Err(e)) => out.push(format!("\n⚠️ Хелпдеск: {e}")),
        None => {}
    }
    out.join("\n")
}

/// A few sentences for the voice: counts and the nearest meeting, no lists.
pub fn briefing_spoken(b: &Briefing) -> String {
    let mut out = Vec::new();
    if let Some(Ok(ev)) = &b.events {
        let timed: Vec<&CalendarEvent> = ev.iter().filter(|e| !e.all_day).collect();
        out.push(match timed.first() {
            None if ev.is_empty() => "Встреч сегодня нет.".to_string(),
            None => format!(
                "Сегодня {}, на весь день.",
                plural(ev.len(), "событие", "события", "событий")
            ),
            Some(first) => format!(
                "Сегодня {}, первая в {} — {}.",
                plural(timed.len(), "встреча", "встречи", "встреч"),
                first.start,
                first.summary
            ),
        });
    }
    if let Some(Ok(m)) = &b.mail {
        out.push(if m.unread == 0 {
            "Непрочитанной почты нет.".to_string()
        } else {
            format!(
                "Непрочитанных писем {}, важных {}.",
                m.unread, m.important_unread
            )
        });
    }
    if let Some(Ok(j)) = &b.jira {
        out.push(if j.is_empty() {
            "В Jira открытых задач нет.".to_string()
        } else {
            format!(
                "В Jira {} в работе.",
                plural(j.len(), "задача", "задачи", "задач")
            )
        });
    }
    if let Some(Ok(t)) = &b.tickets {
        out.push(build_spoken(t));
    }
    let failed = [
        b.events
            .as_ref()
            .is_some_and(|r| r.is_err())
            .then_some("календарь"),
        b.mail
            .as_ref()
            .is_some_and(|r| r.is_err())
            .then_some("почту"),
        b.jira
            .as_ref()
            .is_some_and(|r| r.is_err())
            .then_some("Jira"),
        b.tickets
            .as_ref()
            .is_some_and(|r| r.is_err())
            .then_some("хелпдеск"),
    ];
    let failed: Vec<&str> = failed.into_iter().flatten().collect();
    if !failed.is_empty() {
        out.push(format!("Не удалось проверить: {}.", failed.join(", ")));
    }
    out.join(" ")
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

    #[test]
    fn plural_forms() {
        assert_eq!(plural(1, "встреча", "встречи", "встреч"), "1 встреча");
        assert_eq!(plural(3, "встреча", "встречи", "встреч"), "3 встречи");
        assert_eq!(plural(11, "встреча", "встречи", "встреч"), "11 встреч");
        assert_eq!(plural(22, "задача", "задачи", "задач"), "22 задачи");
    }

    #[test]
    fn briefing_with_failures() {
        let b = Briefing {
            events: Some(Ok(vec![CalendarEvent {
                start: "10:00".into(),
                end: "10:30".into(),
                summary: "Планёрка".into(),
                all_day: false,
            }])),
            mail: Some(Err("Gmail не принял вход".into())),
            jira: Some(Ok(vec![])),
            tickets: None,
        };
        let text = briefing_text(&b, "19.10.2026");
        assert!(
            text.contains("📅 Календарь (1):\n• 10:00–10:30 Планёрка"),
            "{text}"
        );
        assert!(text.contains("⚠️ Почта: Gmail не принял вход"), "{text}");
        assert!(!text.contains("Хелпдеск"));
        assert_eq!(
            briefing_spoken(&b),
            "Сегодня 1 встреча, первая в 10:00 — Планёрка. В Jira открытых задач нет. Не удалось проверить: почту."
        );
    }

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
