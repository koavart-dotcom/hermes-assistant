//! FreeScout helpdesk, read-only: tickets assigned to the user and the
//! unassigned queue, for the "Мои тикеты" window and the quick command
//! "мои тикеты". Credentials come from Hermes' `.env` (`FREESCOUT_URL`,
//! `FREESCOUT_API_KEY`, `FREESCOUT_MY_USER_ID`), the same ones the Hermes
//! skills use. Quirks learned on the Mac stage: the API returns deleted
//! conversations (`state: deleted`) in lists, so they are dropped here.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::{CoreError, Layout, Result};

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub api_key: String,
    pub my_user_id: Option<u64>,
}

/// Read the FreeScout settings from Hermes' `.env`; `None` when not configured.
pub fn config(layout: &Layout) -> Result<Option<Config>> {
    let text = match std::fs::read_to_string(layout.env_file()) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let kv: HashMap<String, String> = crate::envkeys::parse(&text).into_iter().collect();
    let url = kv
        .get("FREESCOUT_URL")
        .map(|s| s.trim_end_matches('/').to_string());
    let api_key = kv.get("FREESCOUT_API_KEY").cloned();
    let (Some(url), Some(api_key)) = (url, api_key) else {
        return Ok(None);
    };
    if url.is_empty() || api_key.is_empty() {
        return Ok(None);
    }
    let my_user_id = kv
        .get("FREESCOUT_MY_USER_ID")
        .and_then(|s| s.trim().parse().ok());
    Ok(Some(Config {
        url,
        api_key,
        my_user_id,
    }))
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Ticket {
    #[ts(type = "number")]
    pub id: u64,
    #[ts(type = "number")]
    pub number: u64,
    pub subject: String,
    /// `active` | `pending` | `closed` | `spam`
    pub status: String,
    pub mailbox: String,
    pub customer: String,
    pub customer_email: String,
    pub assignee: String,
    pub tags: Vec<String>,
    /// ISO-8601 UTC as returned by the API.
    pub created_at: String,
    pub updated_at: String,
    /// Last customer message, HTML stripped, ≤ 280 chars.
    pub last_message: String,
    pub url: String,
    /// `new` (created today) | `updated` (touched today) | `working`
    pub mark: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct TicketsSnapshot {
    /// Unix seconds.
    #[ts(type = "number")]
    pub fetched_at: u64,
    #[ts(type = "number | null")]
    pub my_user_id: Option<u64>,
    /// Assigned to me, active + pending.
    pub mine: Vec<Ticket>,
    /// Nobody's, active + pending.
    pub unassigned: Vec<Ticket>,
    /// One spoken sentence for the voice reaction.
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Conversation {
    #[ts(type = "number")]
    pub id: u64,
    pub ticket: Ticket,
    /// Chronological messages as `[date] Кто:\ntext` blocks.
    pub transcript: String,
}

pub struct Client {
    cfg: Config,
    http: reqwest::Client,
}

impl Client {
    pub fn new(cfg: Config) -> Client {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client");
        Client { cfg, http }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let mut url = format!("{}/api/{path}", self.cfg.url);
        for (i, (k, v)) in query.iter().enumerate() {
            url.push(if i == 0 { '?' } else { '&' });
            url.push_str(k);
            url.push('=');
            url.push_str(&url_encode(v));
        }
        let resp = self
            .http
            .get(&url)
            .header("X-FreeScout-API-Key", &self.cfg.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| CoreError::Other(format!("FreeScout {path}: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(CoreError::Other(format!(
                "FreeScout {path}: HTTP {status} {}",
                body.chars().take(200).collect::<String>()
            )));
        }
        resp.json()
            .await
            .map_err(|e| CoreError::Other(format!("FreeScout {path}: bad JSON: {e}")))
    }

    async fn mailboxes(&self) -> HashMap<u64, String> {
        let mut out = HashMap::new();
        if let Ok(v) = self.get("mailboxes", &[]).await {
            for m in v["_embedded"]["mailboxes"].as_array().into_iter().flatten() {
                if let (Some(id), Some(name)) = (m["id"].as_u64(), m["name"].as_str()) {
                    out.insert(id, name.to_string());
                }
            }
        }
        out
    }

    async fn conversations(&self, assigned_to: &str) -> Result<Vec<Value>> {
        let v = self
            .get(
                "conversations",
                &[
                    ("status", "active,pending".to_string()),
                    ("assignedTo", assigned_to.to_string()),
                    ("pageSize", "100".to_string()),
                    ("sortField", "updatedAt".to_string()),
                    ("sortOrder", "desc".to_string()),
                    ("embed", "threads,tags".to_string()),
                ],
            )
            .await?;
        Ok(v["_embedded"]["conversations"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|c| c["state"].as_str().unwrap_or("published") == "published")
            .collect())
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = format!("{}/api/{path}", self.cfg.url);
        let resp = self
            .http
            .post(&url)
            .header("X-FreeScout-API-Key", &self.cfg.api_key)
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| CoreError::Other(format!("FreeScout {path}: {e}")))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(CoreError::Other(format!(
                "FreeScout {path}: HTTP {status} {}",
                text.chars().take(200).collect::<String>()
            )));
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    /// Conversation id for a ticket number (FreeScout numbers usually equal
    /// ids, but not always): try the `number` filter, then the id itself.
    pub async fn find_by_number(&self, number: u64) -> Result<u64> {
        let v = self
            .get(
                "conversations",
                &[
                    ("number", number.to_string()),
                    ("pageSize", "5".to_string()),
                ],
            )
            .await?;
        if let Some(c) = v["_embedded"]["conversations"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["number"].as_u64() == Some(number))
        {
            if let Some(id) = c["id"].as_u64() {
                return Ok(id);
            }
        }
        let c = self.get(&format!("conversations/{number}"), &[]).await?;
        c["id"]
            .as_u64()
            .ok_or_else(|| CoreError::Other(format!("заявка #{number} не найдена")))
    }

    /// Conversation with its threads, as readable text for the model.
    pub async fn conversation_text(&self, number: u64) -> Result<Conversation> {
        let id = self.find_by_number(number).await?;
        let c = self
            .get(
                &format!("conversations/{id}"),
                &[("embed", "threads".to_string())],
            )
            .await?;
        let boxes = self.mailboxes().await;
        let today = iso_date(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        );
        let ticket = compact(&c, &self.cfg.url, &boxes, &today);
        let mut threads: Vec<Value> = c["_embedded"]["threads"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        // API returns newest first; the model wants chronological order.
        threads.sort_by(|a, b| {
            a["createdAt"]
                .as_str()
                .unwrap_or("")
                .cmp(b["createdAt"].as_str().unwrap_or(""))
        });
        let mut lines = Vec::new();
        for t in threads.iter().filter(|t| {
            matches!(
                t["type"].as_str(),
                Some("customer") | Some("message") | Some("note")
            )
        }) {
            let who = match t["type"].as_str() {
                Some("customer") => format!("Клиент ({})", name_of(&t["customer"])),
                Some("note") => format!("Заметка ({})", name_of(&t["createdBy"])),
                _ => format!("Поддержка ({})", name_of(&t["createdBy"])),
            };
            let body = strip_html(
                t["body"].as_str().or(t["text"].as_str()).unwrap_or(""),
                1500,
            );
            if body.is_empty() {
                continue;
            }
            lines.push(format!(
                "[{}] {who}:\n{body}",
                t["createdAt"]
                    .as_str()
                    .unwrap_or("")
                    .chars()
                    .take(16)
                    .collect::<String>()
            ));
        }
        Ok(Conversation {
            id,
            ticket,
            transcript: lines.join("\n\n"),
        })
    }

    /// Post a reply to the customer as `my_user_id`.
    pub async fn reply(&self, conversation_id: u64, text: &str) -> Result<()> {
        let user = self
            .cfg
            .my_user_id
            .ok_or_else(|| CoreError::Other("FREESCOUT_MY_USER_ID не задан".into()))?;
        let body = serde_json::json!({
            "type": "message",
            "text": text.replace('\n', "<br>"),
            "user": user,
            "imported": false,
        });
        self.post(&format!("conversations/{conversation_id}/threads"), &body)
            .await?;
        Ok(())
    }

    pub async fn snapshot(&self) -> Result<TicketsSnapshot> {
        let boxes = self.mailboxes().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let today = iso_date(now);
        let mine = match self.cfg.my_user_id {
            Some(id) => self
                .conversations(&id.to_string())
                .await?
                .iter()
                .map(|c| compact(c, &self.cfg.url, &boxes, &today))
                .collect(),
            None => Vec::new(),
        };
        let unassigned: Vec<Ticket> = self
            .conversations("")
            .await?
            .iter()
            .map(|c| compact(c, &self.cfg.url, &boxes, &today))
            .collect();
        let summary = summary(&mine, &unassigned, self.cfg.my_user_id.is_some());
        Ok(TicketsSnapshot {
            fetched_at: now,
            my_user_id: self.cfg.my_user_id,
            mine,
            unassigned,
            summary,
        })
    }
}

fn name_of(v: &Value) -> String {
    let parts: Vec<&str> = ["firstName", "lastName"]
        .iter()
        .filter_map(|k| v[*k].as_str())
        .filter(|s| !s.trim().is_empty())
        .collect();
    if parts.is_empty() {
        v["email"].as_str().unwrap_or("").to_string()
    } else {
        parts.join(" ")
    }
}

pub fn compact(c: &Value, base_url: &str, boxes: &HashMap<u64, String>, today: &str) -> Ticket {
    let threads = c["_embedded"]["threads"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let last = threads
        .iter()
        .rev()
        .find(|t| t["type"].as_str() == Some("customer"))
        .or_else(|| threads.last())
        .cloned()
        .unwrap_or(Value::Null);
    let created_at = c["createdAt"].as_str().unwrap_or("").to_string();
    let updated_at = c["updatedAt"].as_str().unwrap_or("").to_string();
    let mark = if created_at.starts_with(today) {
        "new"
    } else if updated_at.starts_with(today) {
        "updated"
    } else {
        "working"
    };
    let id = c["id"].as_u64().unwrap_or(0);
    let mailbox_id = c["mailboxId"].as_u64().unwrap_or(0);
    Ticket {
        id,
        number: c["number"].as_u64().unwrap_or(id),
        subject: c["subject"].as_str().unwrap_or("(без темы)").to_string(),
        status: c["status"].as_str().unwrap_or("").to_string(),
        mailbox: boxes
            .get(&mailbox_id)
            .cloned()
            .unwrap_or_else(|| mailbox_id.to_string()),
        customer: name_of(&c["customer"]),
        customer_email: c["customer"]["email"].as_str().unwrap_or("").to_string(),
        assignee: name_of(&c["assignee"]),
        tags: c["_embedded"]["tags"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect(),
        created_at,
        updated_at,
        last_message: strip_html(
            last["body"]
                .as_str()
                .or(last["text"].as_str())
                .unwrap_or(""),
            280,
        ),
        url: format!("{base_url}/conversation/{id}"),
        mark: mark.to_string(),
    }
}

/// Prompt for the model to draft a reply (the answer becomes the draft).
pub fn draft_prompt(c: &Conversation) -> String {
    format!(
        "Ты помощник IT-поддержки компании Deka. Ниже переписка по заявке #{n} «{subj}» от клиента {cust} (ящик {mb}). \
Напиши черновик ответа клиенту от имени Артема: на языке переписки (украинский или русский), вежливо и по делу, \
3–8 предложений, без обращения «Уважаемый» и без подписи. Если для решения не хватает данных — задай клиенту \
уточняющие вопросы. Выведи только текст ответа, без пояснений.\n\nПереписка:\n{tr}",
        n = c.ticket.number,
        subj = c.ticket.subject,
        cust = if c.ticket.customer.is_empty() {
            c.ticket.customer_email.clone()
        } else {
            c.ticket.customer.clone()
        },
        mb = c.ticket.mailbox,
        tr = c.transcript
    )
}

/// Spoken summary: counts only, short enough for a voice reaction.
pub fn summary(mine: &[Ticket], unassigned: &[Ticket], has_user: bool) -> String {
    fn tickets(n: usize) -> &'static str {
        match (n % 10, n % 100) {
            (1, x) if x != 11 => "заявка",
            (2..=4, x) if !(12..=14).contains(&x) => "заявки",
            _ => "заявок",
        }
    }
    let mut parts = Vec::new();
    if has_user {
        if mine.is_empty() {
            parts.push("На вас нет открытых заявок".to_string());
        } else {
            let fresh = mine.iter().filter(|t| t.mark != "working").count();
            let mut s = format!("На вас {} {}", mine.len(), tickets(mine.len()));
            if fresh > 0 {
                s.push_str(&format!(", {fresh} с новыми сообщениями за сегодня"));
            }
            parts.push(s);
        }
    }
    if !unassigned.is_empty() {
        parts.push(format!(
            "без исполнителя {} {}",
            unassigned.len(),
            tickets(unassigned.len())
        ));
    }
    if parts.is_empty() {
        "Открытых заявок нет".to_string()
    } else {
        parts.join(", ") + "."
    }
}

/// Tags out, entities decoded, whitespace folded, cut at `limit` chars.
pub fn strip_html(s: &str, limit: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut tag = String::new();
    let mut skip_depth: usize = 0; // inside <style>/<script>
    for ch in s.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let t = tag.trim().to_lowercase();
                let name: String = t
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                if name == "style" || name == "script" {
                    if t.starts_with('/') {
                        skip_depth = skip_depth.saturating_sub(1);
                    } else {
                        skip_depth += 1;
                    }
                } else if matches!(name.as_str(), "br" | "p" | "div" | "li" | "tr") {
                    out.push('\n');
                }
                tag.clear();
            } else {
                tag.push(ch);
            }
        } else if ch == '<' {
            in_tag = true;
        } else if skip_depth == 0 {
            out.push(ch);
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut folded = String::new();
    let mut prev_space = false;
    let mut prev_nl = false;
    for ch in decoded.chars() {
        if ch == '\n' {
            if !prev_nl {
                while folded.ends_with(' ') {
                    folded.pop();
                }
                folded.push('\n');
            }
            prev_nl = true;
            prev_space = false;
        } else if ch.is_whitespace() {
            if !prev_space && !prev_nl {
                folded.push(' ');
            }
            prev_space = true;
        } else {
            folded.push(ch);
            prev_space = false;
            prev_nl = false;
        }
    }
    let t = folded.trim();
    if t.chars().count() > limit {
        let cut: String = t.chars().take(limit).collect();
        format!("{}…", cut.trim_end())
    } else {
        t.to_string()
    }
}

/// Percent-encode a query value (RFC 3986 unreserved kept as is).
pub fn url_encode(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `YYYY-MM-DD` (UTC) for a unix timestamp, without a date crate.
pub fn iso_date(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dates_and_html() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(1_792_000_000), "2026-10-14");
        assert_eq!(
            strip_html(
                "<p>Привет,&nbsp;<b>мир</b></p><style>x{}</style><br>второй  абзац",
                100
            ),
            "Привет, мир\nвторой абзац"
        );
        assert_eq!(strip_html("abcdef", 3), "abc…");
        assert_eq!(url_encode("active,pending"), "active%2Cpending");
        assert_eq!(url_encode("threads,tags"), "threads%2Ctags");
    }

    #[test]
    fn compacts_conversation_and_marks_today() {
        let boxes = HashMap::from([(1, "IT Support".to_string())]);
        let c = json!({
            "id": 500, "number": 1234, "subject": "Не печатает принтер", "status": "active",
            "state": "published", "mailboxId": 1,
            "createdAt": "2026-10-16T08:00:00Z", "updatedAt": "2026-10-16T09:00:00Z",
            "customer": {"firstName": "Иван", "lastName": "Петров", "email": "ivan@x.ua"},
            "assignee": {"firstName": "Artem", "lastName": "Kovalenko"},
            "_embedded": {"threads": [
                {"type": "customer", "body": "<p>Принтер <b>не печатает</b></p>"},
                {"type": "message", "body": "Смотрю"}
            ], "tags": [{"name": "printer"}]}
        });
        let t = compact(&c, "https://h.example", &boxes, "2026-10-16");
        assert_eq!(t.number, 1234);
        assert_eq!(t.mailbox, "IT Support");
        assert_eq!(t.customer, "Иван Петров");
        assert_eq!(t.assignee, "Artem Kovalenko");
        assert_eq!(t.last_message, "Принтер не печатает");
        assert_eq!(t.url, "https://h.example/conversation/500");
        assert_eq!(t.mark, "new");
        assert_eq!(t.tags, vec!["printer"]);
        let t2 = compact(&c, "https://h.example", &boxes, "2026-10-17");
        assert_eq!(t2.mark, "working");
    }

    #[test]
    fn draft_prompt_mentions_ticket() {
        let boxes = HashMap::new();
        let c = json!({"id": 9, "number": 4210, "subject": "Нет интернета", "state": "published",
            "customer": {"firstName": "Олег", "email": "o@x.ua"}, "createdAt": "", "updatedAt": "", "_embedded": {}});
        let conv = Conversation {
            id: 9,
            ticket: compact(&c, "https://h", &boxes, "2026-10-16"),
            transcript: "[2026-10-16T08:00] Клиент (Олег):\nНе работает интернет".into(),
        };
        let p = draft_prompt(&conv);
        assert!(p.contains("#4210 «Нет интернета»"));
        assert!(p.contains("клиента Олег"));
        assert!(p.ends_with("Не работает интернет"));
    }

    #[test]
    fn summary_wording() {
        let mk = |mark: &str| Ticket {
            id: 1,
            number: 1,
            subject: String::new(),
            status: "active".into(),
            mailbox: String::new(),
            customer: String::new(),
            customer_email: String::new(),
            assignee: String::new(),
            tags: vec![],
            created_at: String::new(),
            updated_at: String::new(),
            last_message: String::new(),
            url: String::new(),
            mark: mark.into(),
        };
        assert_eq!(summary(&[], &[], true), "На вас нет открытых заявок.");
        assert_eq!(summary(&[], &[], false), "Открытых заявок нет");
        let mine = vec![
            mk("new"),
            mk("working"),
            mk("working"),
            mk("working"),
            mk("working"),
        ];
        assert_eq!(
            summary(&mine, &[mk("working")], true),
            "На вас 5 заявок, 1 с новыми сообщениями за сегодня, без исполнителя 1 заявка."
        );
        assert_eq!(summary(&mine[1..3], &[], true), "На вас 2 заявки.");
    }

    #[test]
    fn config_from_env() {
        let root = std::env::temp_dir().join(format!("hermes-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        assert!(config(&layout).unwrap().is_none());
        std::fs::create_dir_all(&layout.hermes_home).unwrap();
        std::fs::write(
            layout.env_file(),
            "FREESCOUT_URL=https://h.example/\nFREESCOUT_API_KEY=k\nFREESCOUT_MY_USER_ID=4\n",
        )
        .unwrap();
        let c = config(&layout).unwrap().unwrap();
        assert_eq!(c.url, "https://h.example");
        assert_eq!(c.my_user_id, Some(4));
        let _ = std::fs::remove_dir_all(&root);
    }
}
