//! Configuring the installed Hermes Agent for our shell. Everything goes
//! through `hermes config set KEY VALUE`: dotted keys land in config.yaml,
//! UPPER_SNAKE names in .env (Hermes routes them itself).

use tokio_util::sync::CancellationToken;

use crate::cli::run_hermes;
use crate::events::{EventSender, SetupEvent, emit};
use crate::{Layout, Result};

pub const API_SERVER_PORT: u16 = 8642;

/// The settings our shell owns. Keys follow the Hermes config schema
/// (docs: configuration, local-ollama-setup, api-server, security).
/// `ollama_base_url` is `Ollama::base_url()` — the port may differ from
/// 11434 when another program holds it.
pub fn settings(model: &str, api_key: &str, ollama_base_url: &str) -> Vec<(String, String)> {
    let mut s = vec![
        ("model.default".into(), model.into()),
        ("model.provider".into(), "custom".into()),
        ("model.base_url".into(), format!("{ollama_base_url}/v1")),
    ];
    s.extend(base_settings(api_key));
    s
}

/// Settings that do not depend on the model: approvals and the api_server.
/// The server mode applies only these; its model (cloud or Jarvis Deka) is
/// chosen later through `models::apply` (Control API `PUT /models/current`).
pub fn base_settings(api_key: &str) -> Vec<(String, String)> {
    vec![
        ("approvals.mode".into(), "manual".into()),
        ("API_SERVER_ENABLED".into(), "true".into()),
        ("API_SERVER_KEY".into(), api_key.into()),
        ("HERMES_API_TIMEOUT".into(), "1800".into()),
    ]
}

/// Apply [`settings`] to the installation.
pub async fn apply(
    layout: &Layout,
    model: &str,
    api_key: &str,
    ollama_base_url: &str,
    events: &EventSender,
    cancel: &CancellationToken,
) -> Result<()> {
    apply_pairs(
        layout,
        settings(model, api_key, ollama_base_url),
        events,
        cancel,
    )
    .await
}

/// `hermes config set` for each pair; values of `*KEY*` names are not logged.
pub async fn apply_pairs(
    layout: &Layout,
    pairs: Vec<(String, String)>,
    events: &EventSender,
    cancel: &CancellationToken,
) -> Result<()> {
    for (key, value) in pairs {
        let shown = if key.contains("KEY") {
            "***".to_string()
        } else {
            value.clone()
        };
        emit(
            events,
            SetupEvent::Log {
                line: format!("hermes config set {key} {shown}"),
            },
        );
        run_hermes(layout, &["config", "set", &key, &value], None, cancel).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_cover_model_api_and_approvals() {
        let s = settings("qwen3.5", "abc", "http://127.0.0.1:11435");
        let get = |k: &str| s.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("model.default"), Some("qwen3.5"));
        assert_eq!(get("model.provider"), Some("custom"));
        assert_eq!(get("model.base_url"), Some("http://127.0.0.1:11435/v1"));
        assert_eq!(get("approvals.mode"), Some("manual"));
        assert_eq!(get("API_SERVER_KEY"), Some("abc"));
        assert_eq!(get("API_SERVER_ENABLED"), Some("true"));
    }

    #[test]
    fn base_settings_leave_the_model_alone() {
        let s = base_settings("abc");
        assert!(s.iter().all(|(k, _)| !k.starts_with("model.")));
        assert!(
            s.iter()
                .any(|(k, v)| k == "approvals.mode" && v == "manual")
        );
        assert!(s.iter().any(|(k, v)| k == "API_SERVER_KEY" && v == "abc"));
    }
}
