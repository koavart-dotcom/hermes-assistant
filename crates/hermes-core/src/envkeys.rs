//! API keys and service settings for Hermes skills/tools, kept in Hermes'
//! own `.env` (`$HERMES_HOME/.env`, owner-only): what `hermes config set
//! UPPER_SNAKE value` writes, but edited directly so nothing is echoed to a
//! shell. The gateway reads `.env` at start, so changes need a restart.

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CoreError, Layout, Result};

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct EnvKey {
    pub name: String,
    /// `••••1234` — last four characters only; empty for an empty value.
    pub masked: String,
}

/// Known integrations the Settings page offers (name, env vars, hint).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct EnvPreset {
    pub id: String,
    pub title: String,
    pub keys: Vec<String>,
    pub hint: String,
}

pub fn presets() -> Vec<EnvPreset> {
    let p = |id: &str, title: &str, keys: &[&str], hint: &str| EnvPreset {
        id: id.into(),
        title: title.into(),
        keys: keys.iter().map(|k| k.to_string()).collect(),
        hint: hint.into(),
    };
    vec![
        p(
            "freescout",
            "FreeScout (хелпдеск Deka)",
            &[
                "FREESCOUT_URL",
                "FREESCOUT_API_KEY",
                "FREESCOUT_MY_USER_ID",
                "FREESCOUT_MAILBOXES",
            ],
            "Адрес вида https://helpdesk.example.com, ключ API из настроек FreeScout и ваш id пользователя (для фильтра «мои тикеты»). FREESCOUT_MAILBOXES — id ящиков через запятую, по умолчанию 1 (IT Support, support@deka.ua); чужие ящики не показываются.",
        ),
        p(
            "google_calendar",
            "Google Calendar (утренняя сводка)",
            &["GOOGLE_CALENDAR_ICS_URL"],
            "Google Календарь → Настройки → ваш календарь → «Секретный адрес в формате iCal». Несколько календарей — через запятую. Только чтение.",
        ),
        p(
            "gmail",
            "Gmail (утренняя сводка)",
            &["GMAIL_ADDRESS", "GMAIL_APP_PASSWORD"],
            "Адрес и пароль приложения: аккаунт Google → Безопасность → Двухэтапная аутентификация → Пароли приложений. Почта только читается, письма не помечаются прочитанными.",
        ),
        p(
            "jira",
            "Jira (утренняя сводка)",
            &["JIRA_USER", "JIRA_API_TOKEN", "JIRA_SITES"],
            "E-mail в Atlassian и API-токен (id.atlassian.com → Security → API tokens). JIRA_SITES необязателен: по умолчанию deka-admin, deka-dev, deka.",
        ),
        p(
            "telegram",
            "Telegram-бот",
            &["TELEGRAM_BOT_TOKEN", "TELEGRAM_ALLOWED_USERS"],
            "Токен от @BotFather и список разрешённых user id через запятую — канал Telegram в Hermes.",
        ),
        p(
            "anthropic",
            "Anthropic (Claude API)",
            &["ANTHROPIC_API_KEY"],
            "Облачная модель по явному выбору; платно.",
        ),
        p(
            "openai",
            "OpenAI",
            &["OPENAI_API_KEY"],
            "Облачные модели и голос OpenAI; платно.",
        ),
        p(
            "gemini",
            "Google Gemini",
            &["GEMINI_API_KEY"],
            "Модели Google.",
        ),
        p(
            "github",
            "GitHub",
            &["GITHUB_TOKEN"],
            "Токен для инструментов GitHub (репозитории, issues).",
        ),
    ]
}

pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    (2..=64).contains(&b.len())
        && b[0].is_ascii_uppercase()
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

pub fn mask(value: &str) -> String {
    let v = value.trim();
    if v.is_empty() {
        return String::new();
    }
    let tail: String = v
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••{tail}")
}

/// Parse `KEY=value` lines (quotes stripped); comments and blanks skipped.
pub fn parse(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            let t = t.strip_prefix("export ").unwrap_or(t);
            let (k, v) = t.split_once('=')?;
            let k = k.trim();
            if !valid_name(k) {
                return None;
            }
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(v);
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Rewrite `text` with `name=value` (replacing an existing line, appending
/// otherwise); `None` removes the key. Everything else is kept verbatim.
pub fn upsert(text: &str, name: &str, value: Option<&str>) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut done = false;
    for line in text.lines() {
        let t = line.trim();
        let key = t
            .strip_prefix("export ")
            .unwrap_or(t)
            .split_once('=')
            .map(|(k, _)| k.trim());
        if key == Some(name) && !t.starts_with('#') {
            if let Some(v) = value {
                if !done {
                    out.push(format!("{name}={}", quote(v)));
                }
            }
            done = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        if let Some(v) = value {
            out.push(format!("{name}={}", quote(v)));
        }
    }
    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

fn quote(v: &str) -> String {
    if v.chars()
        .any(|c| c.is_whitespace() || c == '#' || c == '"' || c == '\'')
    {
        format!("\"{}\"", v.replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

fn read(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

/// One variable from Hermes' `.env` (trimmed, empty → `None`).
pub fn value(layout: &Layout, name: &str) -> Option<String> {
    let text = std::fs::read_to_string(layout.env_file()).ok()?;
    parse(&text)
        .into_iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub fn list(layout: &Layout) -> Result<Vec<EnvKey>> {
    let text = read(&layout.env_file())?;
    Ok(parse(&text)
        .into_iter()
        .map(|(name, value)| EnvKey {
            masked: mask(&value),
            name,
        })
        .collect())
}

pub fn set(layout: &Layout, name: &str, value: &str) -> Result<()> {
    if !valid_name(name) {
        return Err(CoreError::Other(format!(
            "invalid key name `{name}` (A-Z, 0-9, _)"
        )));
    }
    if value.contains('\n') {
        return Err(CoreError::Other("value must be one line".into()));
    }
    let path = layout.env_file();
    let text = read(&path)?;
    crate::control::write_with_backup(&path, &upsert(&text, name, Some(value.trim())))
}

pub fn remove(layout: &Layout, name: &str) -> Result<()> {
    if !valid_name(name) {
        return Err(CoreError::Other(format!("invalid key name `{name}`")));
    }
    let path = layout.env_file();
    let text = read(&path)?;
    crate::control::write_with_backup(&path, &upsert(&text, name, None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_masks() {
        let text = "# comment\nAPI_SERVER_KEY=abcd1234\nexport FREESCOUT_URL=\"https://h.example.com\"\nbad-name=1\n\nEMPTY=\n";
        let kv = parse(text);
        assert_eq!(kv.len(), 3);
        assert_eq!(
            kv[1],
            ("FREESCOUT_URL".into(), "https://h.example.com".into())
        );
        assert_eq!(mask("abcd1234"), "••••1234");
        assert_eq!(mask("ab"), "••••ab");
        assert_eq!(mask(""), "");
        assert!(valid_name("FREESCOUT_API_KEY"));
        assert!(!valid_name("freescout"));
        assert!(!valid_name("X"));
    }

    #[test]
    fn upsert_replaces_appends_and_removes_keeping_the_rest() {
        let text = "# keep me\nA=1\nB=2\n";
        let t1 = upsert(text, "B", Some("new value"));
        assert_eq!(t1, "# keep me\nA=1\nB=\"new value\"\n");
        let t2 = upsert(&t1, "C", Some("3"));
        assert!(t2.ends_with("C=3\n"));
        let t3 = upsert(&t2, "A", None);
        assert_eq!(t3, "# keep me\nB=\"new value\"\nC=3\n");
        assert_eq!(upsert("", "K", Some("v")), "K=v\n");
    }

    #[test]
    fn set_and_remove_on_disk() {
        let root = std::env::temp_dir().join(format!("hermes-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        std::fs::create_dir_all(&layout.hermes_home).unwrap();
        set(&layout, "FREESCOUT_API_KEY", " secret-xyz ").unwrap();
        set(&layout, "FREESCOUT_URL", "https://h").unwrap();
        let l = list(&layout).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].masked, "••••-xyz");
        assert!(set(&layout, "lower", "x").is_err());
        remove(&layout, "FREESCOUT_URL").unwrap();
        assert_eq!(list(&layout).unwrap().len(), 1);
        assert!(presets().iter().any(|p| p.id == "freescout"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
