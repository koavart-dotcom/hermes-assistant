//! Model / speech settings of Hermes as the Settings page shows them: what is
//! configured now (parsed from `config.yaml`), the choices we offer, and
//! applying a choice with `hermes config set` (the gateway or the voice
//! server is restarted by the caller).

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::cli::run_hermes;
use crate::{CoreError, Layout, Result};

/// Minimal YAML lookup by indentation: `yaml_get(text, &["tts","piper","voice"])`.
/// Enough for Hermes' config.yaml (plain `key: value` maps, no flow syntax).
pub fn yaml_get(text: &str, path: &[&str]) -> Option<String> {
    let mut depth = 0usize;
    // Indent of the key matched at the current depth; its children are the
    // lines with the first deeper indent that follows.
    let mut parent_indent: Option<usize> = None;
    let mut child_indent: Option<usize> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim_end();
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim_start();
        let Some((key, value)) = body.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').trim_matches('\'');
        match parent_indent {
            None => {
                if indent != 0 {
                    continue;
                }
            }
            Some(pi) => {
                if indent <= pi {
                    return None; // the section ended without the key
                }
                let ci = *child_indent.get_or_insert(indent);
                if indent != ci {
                    continue; // deeper nesting of another child
                }
            }
        }
        if key == path[depth] {
            if depth + 1 == path.len() {
                return Some(value.to_string());
            }
            depth += 1;
            parent_indent = Some(indent);
            child_indent = None;
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ModelSettings {
    /// `custom` (OpenAI-compatible: Ollama, Jarvis Deka), `anthropic`, `openai`, `gemini`, …
    pub provider: String,
    pub model: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct SpeechSettings {
    pub stt_provider: String,
    pub stt_model: String,
    pub stt_language: String,
    pub tts_provider: String,
    /// Voice for the selected provider (piper voice id, edge voice name, openai voice, elevenlabs voice id).
    pub tts_voice: String,
}

/// A selectable model shown on the Settings page.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ModelOption {
    pub id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// Name of the `.env` key the provider needs (empty for local).
    pub needs_key: String,
    /// The provider is usable now: no key needed, the key is set, or a
    /// subscription login (OAuth) is stored by Hermes.
    pub ready: bool,
    pub hint: String,
}

/// Hermes stores its own Anthropic OAuth grant here (`hermes auth add anthropic --type oauth`).
pub fn anthropic_oauth_present(layout: &Layout) -> bool {
    layout.hermes_home.join(".anthropic_oauth.json").is_file()
        || layout.hermes_home.join("auth.json").is_file()
}

/// Fill `ready` from the `.env` keys and the OAuth files of a layout.
pub fn mark_ready(opts: &mut [ModelOption], layout: &Layout) {
    let env = std::fs::read_to_string(layout.env_file()).unwrap_or_default();
    let keys: std::collections::HashMap<String, String> =
        crate::envkeys::parse(&env).into_iter().collect();
    let oauth = anthropic_oauth_present(layout);
    for o in opts.iter_mut() {
        o.ready = o.needs_key.is_empty()
            || keys.get(&o.needs_key).is_some_and(|v| !v.trim().is_empty())
            || (o.provider == "anthropic" && oauth);
    }
}

pub fn options(ollama_models: &[String], ollama_base_url: &str) -> Vec<ModelOption> {
    let mut out: Vec<ModelOption> = ollama_models
        .iter()
        .map(|m| ModelOption {
            id: format!("ollama:{m}"),
            title: format!("{m} (локально, Ollama)"),
            provider: "custom".into(),
            model: m.clone(),
            base_url: format!("{ollama_base_url}/v1"),
            needs_key: String::new(),
            ready: true,
            hint: "Работает на вашей видеокарте, без интернета.".into(),
        })
        .collect();
    out.push(ModelOption {
        id: "jarvis-deka".into(),
        title: "Jarvis Deka (qwen3.8-27b, сеть Deka)".into(),
        provider: "custom".into(),
        model: "qwen3.8-27b".into(),
        base_url: "http://172.15.15.125:8000/v1".into(),
        needs_key: String::new(),
        ready: true,
        hint: "Сервер Deka; доступен только из сети компании.".into(),
    });
    out.push(ModelOption {
        id: "anthropic".into(),
        title: "Anthropic Claude (облако)".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-5-5".into(),
        base_url: String::new(),
        needs_key: "ANTHROPIC_API_KEY".into(),
        ready: false,
        hint: "Ключ API в «Интеграции и ключи» (платно) или вход по подписке Claude Max: `hermes auth add anthropic --type oauth`.".into(),
    });
    out.push(ModelOption {
        id: "openai".into(),
        title: "OpenAI (облако)".into(),
        provider: "openai".into(),
        model: "gpt-5".into(),
        base_url: String::new(),
        needs_key: "OPENAI_API_KEY".into(),
        ready: false,
        hint: "Платно, нужен ключ в «Интеграции и ключи».".into(),
    });
    out.push(ModelOption {
        id: "gemini".into(),
        title: "Google Gemini (облако)".into(),
        provider: "gemini".into(),
        model: "gemini-2.5-pro".into(),
        base_url: String::new(),
        needs_key: "GEMINI_API_KEY".into(),
        ready: false,
        hint: "Платно, нужен ключ в «Интеграции и ключи».".into(),
    });
    out
}

pub const STT_MODELS: &[&str] = &[
    "tiny",
    "base",
    "small",
    "medium",
    "large-v3",
    "large-v3-turbo",
];
pub const STT_LANGUAGES: &[&str] = &["ru", "uk", "en", "auto"];

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct TtsProviderOption {
    pub id: String,
    pub title: String,
    pub voices: Vec<String>,
    pub needs_key: String,
    pub hint: String,
}

pub fn tts_options() -> Vec<TtsProviderOption> {
    let p = |id: &str, title: &str, voices: &[&str], key: &str, hint: &str| TtsProviderOption {
        id: id.into(),
        title: title.into(),
        voices: voices.iter().map(|v| v.to_string()).collect(),
        needs_key: key.into(),
        hint: hint.into(),
    };
    vec![
        p(
            "jarvis",
            "Джарвис (локально, клон голоса XTTS)",
            &["Jarvis"],
            "",
            "Голос Джарвиса по образцам из набора реакций, на видеокарте. Ставится отдельно: tools/jarvis-tts/install.ps1.",
        ),
        p(
            "piper",
            "Piper (офлайн)",
            &[
                "ru_RU-dmitri-medium",
                "ru_RU-irina-medium",
                "ru_RU-denis-medium",
                "ru_RU-ruslan-medium",
                "uk_UA-ukrainian_tts-medium",
                "en_US-lessac-medium",
                "en_GB-alan-medium",
            ],
            "",
            "Голос скачивается при первом использовании (~60 МБ), работает без интернета.",
        ),
        p(
            "edge",
            "Microsoft Edge (облако, бесплатно)",
            &[
                "ru-RU-DmitryNeural",
                "ru-RU-SvetlanaNeural",
                "uk-UA-OstapNeural",
                "uk-UA-PolinaNeural",
                "en-US-GuyNeural",
            ],
            "",
            "16.10.2026 отвечал ошибкой 403 — может не работать.",
        ),
        p(
            "openai",
            "OpenAI TTS (облако)",
            &["alloy", "echo", "fable", "onyx", "nova", "shimmer"],
            "OPENAI_API_KEY",
            "Платно, нужен ключ.",
        ),
        p(
            "elevenlabs",
            "ElevenLabs (облако, клонирование голоса)",
            &[],
            "ELEVENLABS_API_KEY",
            "Платно; укажите voice_id своего голоса (например, Джарвиса).",
        ),
        p(
            "gemini",
            "Google Gemini TTS (облако)",
            &["Kore", "Puck", "Charon", "Fenrir", "Aoede"],
            "GEMINI_API_KEY",
            "Платно, нужен ключ.",
        ),
    ]
}

fn read_config(layout: &Layout) -> String {
    std::fs::read_to_string(layout.config_file()).unwrap_or_default()
}

pub fn current_model(layout: &Layout) -> ModelSettings {
    let t = read_config(layout);
    ModelSettings {
        provider: yaml_get(&t, &["model", "provider"]).unwrap_or_default(),
        model: yaml_get(&t, &["model", "default"]).unwrap_or_default(),
        base_url: yaml_get(&t, &["model", "base_url"]).unwrap_or_default(),
    }
}

pub fn current_speech(layout: &Layout) -> SpeechSettings {
    let t = read_config(layout);
    let tts_provider = yaml_get(&t, &["tts", "provider"]).unwrap_or_else(|| "piper".into());
    let voice_key: &[&str] = match tts_provider.as_str() {
        "jarvis" => &["tts", "providers", "jarvis", "voice"],
        "piper" => &["tts", "piper", "voice"],
        "edge" => &["tts", "edge", "voice"],
        "openai" => &["tts", "openai", "voice"],
        "elevenlabs" => &["tts", "elevenlabs", "voice_id"],
        "gemini" => &["tts", "gemini", "voice"],
        _ => &["tts", "piper", "voice"],
    };
    SpeechSettings {
        stt_provider: yaml_get(&t, &["stt", "provider"]).unwrap_or_else(|| "local".into()),
        stt_model: yaml_get(&t, &["stt", "local", "model"]).unwrap_or_else(|| "small".into()),
        stt_language: yaml_get(&t, &["stt", "local", "language"]).unwrap_or_else(|| "ru".into()),
        tts_provider,
        tts_voice: yaml_get(&t, voice_key).unwrap_or_default(),
    }
}

fn valid_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:/".contains(c))
}

async fn set_many(layout: &Layout, pairs: &[(String, String)]) -> Result<()> {
    let cancel = CancellationToken::new();
    for (k, v) in pairs {
        log::info!("hermes config set {k} {v}");
        run_hermes(layout, &["config", "set", k, v], None, &cancel).await?;
    }
    Ok(())
}

/// Write the model choice; the caller restarts the gateway.
pub async fn apply_model(layout: &Layout, s: &ModelSettings) -> Result<()> {
    if !valid_token(&s.model) || !valid_token(&s.provider) {
        return Err(CoreError::Other(
            "недопустимое имя модели или провайдера".into(),
        ));
    }
    let mut pairs = vec![
        ("model.default".to_string(), s.model.clone()),
        ("model.provider".to_string(), s.provider.clone()),
    ];
    if s.provider == "custom" {
        if !(s.base_url.starts_with("http://") || s.base_url.starts_with("https://")) {
            return Err(CoreError::Other(
                "для провайдера custom нужен base_url".into(),
            ));
        }
        pairs.push(("model.base_url".to_string(), s.base_url.clone()));
    } else {
        pairs.push(("model.base_url".to_string(), String::new()));
    }
    set_many(layout, &pairs).await
}

/// Write STT/TTS choices; the caller restarts the voice server.
pub async fn apply_speech(layout: &Layout, s: &SpeechSettings) -> Result<()> {
    if !STT_MODELS.contains(&s.stt_model.as_str()) {
        return Err(CoreError::Other(format!(
            "неизвестная модель распознавания `{}`",
            s.stt_model
        )));
    }
    if !STT_LANGUAGES.contains(&s.stt_language.as_str()) {
        return Err(CoreError::Other(format!(
            "неизвестный язык `{}`",
            s.stt_language
        )));
    }
    let voice_key = match s.tts_provider.as_str() {
        "jarvis" => "tts.providers.jarvis.voice",
        "piper" => "tts.piper.voice",
        "edge" => "tts.edge.voice",
        "openai" => "tts.openai.voice",
        "elevenlabs" => "tts.elevenlabs.voice_id",
        "gemini" => "tts.gemini.voice",
        other => return Err(CoreError::Other(format!("неизвестная озвучка `{other}`"))),
    };
    if !valid_token(&s.tts_voice) {
        return Err(CoreError::Other("недопустимое имя голоса".into()));
    }
    let pairs = vec![
        ("stt.provider".to_string(), "local".to_string()),
        ("stt.local.model".to_string(), s.stt_model.clone()),
        ("stt.local.language".to_string(), s.stt_language.clone()),
        ("tts.provider".to_string(), s.tts_provider.clone()),
        (voice_key.to_string(), s.tts_voice.clone()),
    ];
    set_many(layout, &pairs).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: &str = r#"
# comment
model:
  default: "qwen3.5"
  provider: custom   # local
  base_url: http://127.0.0.1:11437/v1
stt:
  provider: local
  local:
    model: small
    language: ru
tts:
  provider: piper
  edge:
    voice: ru-RU-DmitryNeural
  piper:
    voice: ru_RU-dmitri-medium
agent:
  max_turns: 500
"#;

    #[test]
    fn yaml_lookup_by_indentation() {
        assert_eq!(
            yaml_get(CFG, &["model", "default"]).as_deref(),
            Some("qwen3.5")
        );
        assert_eq!(
            yaml_get(CFG, &["model", "provider"]).as_deref(),
            Some("custom")
        );
        assert_eq!(
            yaml_get(CFG, &["stt", "local", "model"]).as_deref(),
            Some("small")
        );
        assert_eq!(
            yaml_get(CFG, &["tts", "piper", "voice"]).as_deref(),
            Some("ru_RU-dmitri-medium")
        );
        assert_eq!(
            yaml_get(CFG, &["tts", "edge", "voice"]).as_deref(),
            Some("ru-RU-DmitryNeural")
        );
        assert_eq!(
            yaml_get(CFG, &["agent", "max_turns"]).as_deref(),
            Some("500")
        );
        assert_eq!(yaml_get(CFG, &["model", "missing"]), None);
        assert_eq!(yaml_get(CFG, &["nope", "x"]), None);
    }

    #[test]
    fn current_settings_from_config_file() {
        let root = std::env::temp_dir().join(format!("hermes-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        std::fs::create_dir_all(&layout.hermes_home).unwrap();
        std::fs::write(layout.config_file(), CFG).unwrap();
        let m = current_model(&layout);
        assert_eq!(m.model, "qwen3.5");
        assert_eq!(m.base_url, "http://127.0.0.1:11437/v1");
        let s = current_speech(&layout);
        assert_eq!(s.tts_voice, "ru_RU-dmitri-medium");
        assert_eq!(s.stt_model, "small");
        let mut opts = options(&["qwen3.5".into()], "http://127.0.0.1:11437");
        assert_eq!(opts[0].id, "ollama:qwen3.5");
        assert!(opts.iter().any(|o| o.provider == "anthropic"));
        mark_ready(&mut opts, &layout);
        assert!(opts[0].ready);
        assert!(
            !opts
                .iter()
                .find(|o| o.provider == "anthropic")
                .unwrap()
                .ready
        );
        std::fs::write(layout.hermes_home.join(".anthropic_oauth.json"), "{}").unwrap();
        mark_ready(&mut opts, &layout);
        assert!(
            opts.iter()
                .find(|o| o.provider == "anthropic")
                .unwrap()
                .ready
        );
        std::fs::write(layout.env_file(), "OPENAI_API_KEY=sk-x\n").unwrap();
        mark_ready(&mut opts, &layout);
        assert!(opts.iter().find(|o| o.provider == "openai").unwrap().ready);
        assert!(
            tts_options()
                .iter()
                .any(|t| t.id == "piper" && t.voices.len() > 3)
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
