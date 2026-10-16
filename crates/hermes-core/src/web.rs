//! Browser helpers for quick commands that need more than a URL: pick the
//! N-th YouTube video or search result for a query, read a page as text.
//! Done with plain HTTP from the shell (no DevTools attachment: Chrome
//! ignores `--remote-debugging-port` on the default profile since v136, and a
//! separate profile would lose the user's logins). Results open in the
//! user's normal browser.

use std::time::Duration;

use crate::freescout::{strip_html, url_encode};
use crate::{CoreError, Result};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .build()
        .expect("reqwest client")
}

async fn get(url: &str, cookie: Option<&str>) -> Result<String> {
    let mut req = client()
        .get(url)
        .header("Accept-Language", "ru-RU,ru;q=0.9,en;q=0.5");
    if let Some(c) = cookie {
        req = req.header("Cookie", c);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| CoreError::Other(format!("{url}: {e}")))?;
    if !resp.status().is_success() {
        return Err(CoreError::Other(format!("{url}: HTTP {}", resp.status())));
    }
    resp.text()
        .await
        .map_err(|e| CoreError::Other(format!("{url}: {e}")))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub url: String,
    pub title: String,
}

/// Video ids in order of appearance on a YouTube results page (deduplicated).
pub fn parse_youtube_ids(html: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("\"videoId\":\"") {
        rest = &rest[i + 11..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if id.len() == 11 && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

pub async fn youtube_search(query: &str) -> Result<Vec<Hit>> {
    let url = format!(
        "https://www.youtube.com/results?search_query={}&hl=ru",
        url_encode(query)
    );
    let html = get(
        &url,
        Some("CONSENT=YES+cb.20240101-00-p0.ru+FX+000; SOCS=CAI"),
    )
    .await?;
    let ids = parse_youtube_ids(&html);
    if ids.is_empty() {
        return Err(CoreError::Other(format!(
            "YouTube: ничего не найдено по «{query}»"
        )));
    }
    Ok(ids
        .into_iter()
        .map(|id| Hit {
            url: format!("https://www.youtube.com/watch?v={id}"),
            title: String::new(),
        })
        .collect())
}

/// Result links of the DuckDuckGo HTML endpoint (`result__a` anchors with a
/// `uddg=` redirect), in page order.
pub fn parse_ddg(html: &str) -> Vec<Hit> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("class=\"result__a\"") {
        let after = &rest[i..];
        let Some(h) = after.find("href=\"") else {
            break;
        };
        let href = &after[h + 6..];
        let Some(end) = href.find('"') else { break };
        let link = &href[..end];
        let title_start = href[end..].find('>').map(|p| end + p + 1).unwrap_or(end);
        let title_end = href[title_start..]
            .find("</a>")
            .map(|p| title_start + p)
            .unwrap_or(title_start);
        let title = strip_html(&href[title_start..title_end], 200);
        let url = match link.find("uddg=") {
            Some(p) => {
                let enc = &link[p + 5..];
                let enc = enc.split('&').next().unwrap_or(enc);
                url_decode(enc)
            }
            None => link.to_string(),
        };
        if url.starts_with("http") && !out.iter().any(|h: &Hit| h.url == url) {
            out.push(Hit { url, title });
        }
        rest = &href[end..];
    }
    out
}

pub async fn web_search(query: &str) -> Result<Vec<Hit>> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}&kl=ru-ru",
        url_encode(query)
    );
    let hits = parse_ddg(&get(&url, None).await?);
    if hits.is_empty() {
        return Err(CoreError::Other(format!(
            "поиск: ничего не найдено по «{query}»"
        )));
    }
    Ok(hits)
}

/// Page as readable text, at most `limit` characters.
pub async fn page_text(url: &str, limit: usize) -> Result<String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(CoreError::Other(format!("не http(s)-адрес: `{url}`")));
    }
    let html = get(url, None).await?;
    let title = html
        .find("<title")
        .and_then(|i| {
            let t = &html[i..];
            let s = t.find('>')? + 1;
            let e = t.find("</title>")?;
            Some(strip_html(&t[s..e], 200))
        })
        .unwrap_or_default();
    let body = match html.find("<body") {
        Some(i) => &html[i..],
        None => html.as_str(),
    };
    let text = strip_html(body, limit);
    Ok(if title.is_empty() {
        text
    } else {
        format!("{title}\n\n{text}")
    })
}

pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_ids_in_order_without_duplicates() {
        let html = r#"{"videoId":"abc123DEF45","x":1},{"videoId":"abc123DEF45"},{"videoId":"zzz999YYY88","y":2},{"videoId":"short"}"#;
        assert_eq!(parse_youtube_ids(html), vec!["abc123DEF45", "zzz999YYY88"]);
    }

    #[test]
    fn ddg_links_are_decoded() {
        let html = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fru.wikipedia.org%2Fwiki%2F%D0%9A%D0%BE%D1%82&amp;rut=abc">Кот — <b>Википедия</b></a>
        <a class="result__a" href="https://example.com/page">Example</a>"#;
        let hits = parse_ddg(html);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://ru.wikipedia.org/wiki/Кот");
        assert_eq!(hits[0].title, "Кот — Википедия");
        assert_eq!(hits[1].url, "https://example.com/page");
        assert_eq!(url_decode("a%20b+c"), "a b c");
        assert_eq!(url_decode("100%"), "100%");
    }
}
