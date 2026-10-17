//! Unread Gmail for the morning briefing: a minimal read-only IMAP client
//! (`EXAMINE`, never `SELECT`, so nothing is marked as read). Credentials from
//! Hermes' `.env`: `GMAIL_ADDRESS` and `GMAIL_APP_PASSWORD` — an app password
//! (Google account → Security → 2-Step Verification → App passwords), not the
//! account password.

use std::sync::Arc;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::{CoreError, Layout, Result};

pub const ENV_ADDRESS: &str = "GMAIL_ADDRESS";
pub const ENV_PASSWORD: &str = "GMAIL_APP_PASSWORD";
const HOST: &str = "imap.gmail.com";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailItem {
    pub from: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MailSummary {
    pub unread: u32,
    /// Unread messages Gmail marks as important.
    pub important_unread: u32,
    /// Newest important unread (max 5).
    pub important: Vec<MailItem>,
}

pub struct Credentials {
    pub address: String,
    pub password: String,
}

pub fn credentials(layout: &Layout) -> Option<Credentials> {
    Some(Credentials {
        address: crate::envkeys::value(layout, ENV_ADDRESS)?,
        // Google shows app passwords in groups of four: «abcd efgh ijkl mnop».
        password: crate::envkeys::value(layout, ENV_PASSWORD)?.replace(' ', ""),
    })
}

fn err(e: impl std::fmt::Display) -> CoreError {
    CoreError::Other(format!("почта: {e}"))
}

type Stream = BufReader<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>;

struct Imap {
    s: Stream,
    tag: u32,
}

impl Imap {
    async fn connect() -> Result<Self> {
        use rustls_platform_verifier::BuilderVerifierExt;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(err)?
            .with_platform_verifier()
            .map_err(err)?
            .with_no_client_auth();
        let tcp = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio::net::TcpStream::connect((HOST, 993)),
        )
        .await
        .map_err(|_| err("нет соединения с imap.gmail.com"))?
        .map_err(err)?;
        let name = rustls::pki_types::ServerName::try_from(HOST).map_err(err)?;
        let tls = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect(name, tcp)
            .await
            .map_err(err)?;
        let mut imap = Imap {
            s: BufReader::new(tls),
            tag: 0,
        };
        imap.read_line().await?; // greeting
        Ok(imap)
    }

    async fn read_line(&mut self) -> Result<String> {
        let mut buf = Vec::new();
        let n = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.s.read_until(b'\n', &mut buf),
        )
        .await
        .map_err(|_| err("сервер не отвечает"))?
        .map_err(err)?;
        if n == 0 {
            return Err(err("соединение закрыто"));
        }
        Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
    }

    /// Send a command; return the untagged lines with literals inlined.
    async fn cmd(&mut self, command: &str) -> Result<Vec<String>> {
        self.tag += 1;
        let tag = format!("a{}", self.tag);
        self.s
            .get_mut()
            .write_all(format!("{tag} {command}\r\n").as_bytes())
            .await
            .map_err(err)?;
        let mut lines = Vec::new();
        loop {
            let mut line = self.read_line().await?;
            // `{123}` at the end: a literal of that many bytes follows.
            while let Some(len) = literal_len(&line) {
                let mut lit = vec![0u8; len];
                self.s.read_exact(&mut lit).await.map_err(err)?;
                line.push('\n');
                line.push_str(&String::from_utf8_lossy(&lit));
                let rest = self.read_line().await?;
                line.push_str(&rest);
            }
            if let Some(status) = line.strip_prefix(&format!("{tag} ")) {
                if status.starts_with("OK") {
                    return Ok(lines);
                }
                return Err(err(if command.starts_with("LOGIN") {
                    "Gmail не принял вход — нужен пароль приложения (GMAIL_APP_PASSWORD) и включённый IMAP"
                        .to_string()
                } else {
                    status.to_string()
                }));
            }
            lines.push(line);
        }
    }
}

fn literal_len(line: &str) -> Option<usize> {
    let l = line.strip_suffix('}')?;
    let open = l.rfind('{')?;
    l[open + 1..].parse().ok()
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn search_ids(lines: &[String]) -> Vec<u32> {
    lines
        .iter()
        .filter_map(|l| l.strip_prefix("* SEARCH"))
        .flat_map(|rest| rest.split_whitespace().filter_map(|x| x.parse().ok()))
        .collect()
}

pub async fn summary(c: &Credentials) -> Result<MailSummary> {
    let mut imap = Imap::connect().await?;
    imap.cmd(&format!(
        "LOGIN {} {}",
        quote(&c.address),
        quote(&c.password)
    ))
    .await?;
    imap.cmd("EXAMINE INBOX").await?;
    let unread = search_ids(&imap.cmd("SEARCH UNSEEN").await?);
    let mut important = search_ids(
        &imap
            .cmd("SEARCH X-GM-RAW \"is:unread is:important in:inbox\"")
            .await?,
    );
    let important_unread = important.len() as u32;
    important.sort_unstable();
    let newest: Vec<String> = important.iter().rev().take(5).map(u32::to_string).collect();
    let mut items = Vec::new();
    if !newest.is_empty() {
        let lines = imap
            .cmd(&format!(
                "FETCH {} (BODY.PEEK[HEADER.FIELDS (FROM SUBJECT)])",
                newest.join(",")
            ))
            .await?;
        for l in lines.iter().filter(|l| l.contains("FETCH")) {
            items.push(parse_headers(l));
        }
        items.reverse(); // FETCH answers in ascending order; newest first
    }
    let _ = imap.cmd("LOGOUT").await;
    Ok(MailSummary {
        unread: unread.len() as u32,
        important_unread,
        important: items,
    })
}

/// From/Subject out of a FETCH response (headers come as a literal, unfolded here).
fn parse_headers(fetch: &str) -> MailItem {
    let mut from = String::new();
    let mut subject = String::new();
    let mut current: Option<&mut String> = None;
    for raw in fetch.lines() {
        if raw.starts_with(' ') || raw.starts_with('\t') {
            if let Some(c) = current.as_deref_mut() {
                c.push(' ');
                c.push_str(raw.trim());
            }
            continue;
        }
        let lower = raw.to_ascii_lowercase();
        current = if lower.starts_with("from:") {
            from = raw[5..].trim().to_string();
            Some(&mut from)
        } else if lower.starts_with("subject:") {
            subject = raw[8..].trim().to_string();
            Some(&mut subject)
        } else {
            None
        };
    }
    let from = decode_words(&from);
    // «Иван Петров <ivan@x>» → «Иван Петров».
    let from = match from.find('<') {
        Some(i) if i > 0 => from[..i].trim().trim_matches('"').to_string(),
        _ => from.trim_matches(['<', '>']).to_string(),
    };
    MailItem {
        from,
        subject: decode_words(&subject),
    }
}

/// RFC 2047 encoded words (`=?UTF-8?B?...?=`, `=?utf-8?Q?...?=`); other
/// charsets are passed through lossily as UTF-8.
pub fn decode_words(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let before = &rest[..start];
        let after = &rest[start + 2..];
        let parsed = (|| {
            let (_charset, tail) = after.split_once('?')?;
            let (enc, tail) = tail.split_once('?')?;
            let end = tail.find("?=")?;
            let data = &tail[..end];
            let bytes = match enc.to_ascii_uppercase().as_str() {
                "B" => base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .ok()?,
                "Q" => q_decode(data),
                _ => return None,
            };
            let consumed = start + 2 + (after.len() - tail.len()) + end + 2;
            Some((String::from_utf8_lossy(&bytes).to_string(), consumed))
        })();
        match parsed {
            Some((text, consumed)) => {
                // Whitespace between two encoded words is dropped.
                if !(last_was_word && before.trim().is_empty()) {
                    out.push_str(before);
                }
                out.push_str(&text);
                rest = &rest[consumed..];
                last_was_word = true;
            }
            None => {
                out.push_str(&rest[..start + 2]);
                rest = &rest[start + 2..];
                last_was_word = false;
            }
        }
    }
    out.push_str(rest);
    out
}

fn q_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => out.push(b' '),
            b'=' if i + 3 <= b.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(v) => {
                    out.push(v);
                    i += 2;
                }
                Err(_) => out.push(b'='),
            },
            c => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_encoded_words() {
        assert_eq!(
            decode_words("=?UTF-8?B?0J/RgNC40LLQtdGC?= =?UTF-8?B?LCDQkNGA0YLQtdC8?="),
            "Привет, Артем"
        );
        assert_eq!(
            decode_words("=?utf-8?Q?=D0=A1=D1=87=D1=91=D1=82_=E2=84=96?=5"),
            "Счёт №5"
        );
        assert_eq!(decode_words("Plain subject"), "Plain subject");
    }

    #[test]
    fn parses_fetch_headers() {
        let fetch = "* 12 FETCH (BODY[HEADER.FIELDS (FROM SUBJECT)] {80}\nFrom: =?UTF-8?B?0JjQstCw0L0=?= <ivan@deka.ua>\r\nSubject: Отчёт\r\n за неделю\r\n\r\n)";
        let m = parse_headers(fetch);
        assert_eq!(m.from, "Иван");
        assert_eq!(m.subject, "Отчёт за неделю");
    }

    #[test]
    fn literal_and_search() {
        assert_eq!(literal_len("* 1 FETCH (BODY[...] {42}"), Some(42));
        assert_eq!(literal_len("* OK done"), None);
        assert_eq!(search_ids(&["* SEARCH 3 7 12".into()]), vec![3, 7, 12]);
        assert!(search_ids(&["* SEARCH".into()]).is_empty());
    }
}
