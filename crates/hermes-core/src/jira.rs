//! Open Jira issues assigned to Artem for the morning briefing (read-only).
//! Jira Cloud REST v3 with basic auth: `JIRA_USER` (e-mail) + `JIRA_API_TOKEN`
//! from Hermes' `.env` (token: id.atlassian.com → Security → API tokens).
//! Sites: `JIRA_SITES` — subdomains of `*.atlassian.net`, comma-separated;
//! default the three Deka sites (deka-admin ADM, deka-dev DEKA, deka FE).

use serde::{Deserialize, Serialize};

use crate::{CoreError, Layout, Result};

pub const ENV_USER: &str = "JIRA_USER";
pub const ENV_TOKEN: &str = "JIRA_API_TOKEN";
pub const ENV_SITES: &str = "JIRA_SITES";
const DEFAULT_SITES: &str = "deka-admin,deka-dev,deka";
const JQL: &str =
    "assignee = currentUser() AND statusCategory != Done ORDER BY priority DESC, updated DESC";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JiraIssue {
    pub key: String,
    pub summary: String,
    pub status: String,
    pub priority: String,
    pub url: String,
}

pub struct Config {
    pub user: String,
    pub token: String,
    pub sites: Vec<String>,
}

pub fn config(layout: &Layout) -> Option<Config> {
    let sites = crate::envkeys::value(layout, ENV_SITES).unwrap_or_else(|| DEFAULT_SITES.into());
    Some(Config {
        user: crate::envkeys::value(layout, ENV_USER)?,
        token: crate::envkeys::value(layout, ENV_TOKEN)?,
        sites: sites
            .split(',')
            .map(|s| s.trim().trim_end_matches(".atlassian.net").to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    })
}

#[derive(Deserialize)]
struct Search {
    #[serde(default)]
    issues: Vec<RawIssue>,
}

#[derive(Deserialize)]
struct RawIssue {
    key: String,
    fields: Fields,
}

#[derive(Deserialize)]
struct Fields {
    #[serde(default)]
    summary: String,
    status: Option<Named>,
    priority: Option<Named>,
}

#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    name: String,
}

/// My open issues on every site (a site that fails is reported, the rest still count).
pub async fn my_open(cfg: &Config) -> Result<Vec<JiraIssue>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("reqwest client");
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for site in &cfg.sites {
        let base = format!("https://{site}.atlassian.net");
        let resp = client
            .post(format!("{base}/rest/api/3/search/jql"))
            .basic_auth(&cfg.user, Some(&cfg.token))
            .json(&serde_json::json!({
                "jql": JQL,
                "fields": ["summary", "status", "priority"],
                "maxResults": 50,
            }))
            .send()
            .await;
        let resp = match resp {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                errors.push(format!("{site}: HTTP {}", r.status()));
                continue;
            }
            Err(e) => {
                errors.push(format!("{site}: {e}"));
                continue;
            }
        };
        match resp.json::<Search>().await {
            Ok(s) => out.extend(s.issues.into_iter().map(|i| JiraIssue {
                url: format!("{base}/browse/{}", i.key),
                key: i.key,
                summary: i.fields.summary,
                status: i.fields.status.map(|n| n.name).unwrap_or_default(),
                priority: i.fields.priority.map(|n| n.name).unwrap_or_default(),
            })),
            Err(e) => errors.push(format!("{site}: {e}")),
        }
    }
    if out.is_empty() && !errors.is_empty() {
        return Err(CoreError::Other(format!("Jira: {}", errors.join("; "))));
    }
    for e in errors {
        log::warn!("jira: {e}");
    }
    Ok(out)
}

pub fn line(i: &JiraIssue) -> String {
    format!("• {} {} — {}", i.key, i.summary, i.status)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live check: `JIRA_USER=… JIRA_API_TOKEN=… cargo test -p hermes-core jira_live -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn jira_live() {
        let cfg = Config {
            user: std::env::var(ENV_USER).unwrap(),
            token: std::env::var(ENV_TOKEN).unwrap(),
            sites: DEFAULT_SITES.split(',').map(str::to_string).collect(),
        };
        let issues = my_open(&cfg).await.unwrap();
        println!("{} issues", issues.len());
        for i in issues.iter().take(3) {
            println!("{}", line(i));
        }
    }
}
