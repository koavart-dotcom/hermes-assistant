//! Shell preferences (ours, not Hermes'): `settings.json` in the app data
//! dir. Voice volume, silent mode, wake words, whether the wake word is
//! required in always-listening mode, and the reaction voice pack.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{Layout, Result};

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(default)]
pub struct Prefs {
    /// 0..1, applied to reaction clips and spoken answers.
    pub voice_volume: f64,
    /// No clips and no spoken answers at all.
    pub silent: bool,
    /// Words that address the assistant ("джарвис", "гермес").
    pub wake_words: Vec<String>,
    /// Always-listening mode: when false, quick commands run without the
    /// wake word; the model still needs it.
    pub wake_required: bool,
    /// Folder under `voice-pack/` with the reaction clips.
    pub voice_pack: String,
    /// Helpdesk digest schedule.
    pub digest: Digest,
    /// Project name (from the bridge's projects.json) for «попроси Клода …».
    pub claude_project: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(default)]
pub struct Digest {
    pub enabled: bool,
    /// Local times `HH:MM`.
    pub times: Vec<String>,
    /// Say it aloud (respecting silent mode) and post into the voice chat.
    pub speak: bool,
    /// Send to Telegram (needs TELEGRAM_BOT_TOKEN + chat id in Hermes' .env).
    pub telegram: bool,
}

impl Default for Digest {
    fn default() -> Self {
        Digest {
            enabled: false,
            times: vec!["09:00".into(), "14:00".into(), "17:00".into()],
            speak: true,
            telegram: true,
        }
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            voice_volume: 0.8,
            silent: false,
            wake_words: vec![
                "джарвис".into(),
                "гермес".into(),
                "jarvis".into(),
                "hermes".into(),
            ],
            wake_required: true,
            voice_pack: "Jarvis".into(),
            digest: Digest::default(),
            claude_project: "hermes-assistant".into(),
        }
    }
}

pub fn path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("settings.json")
}

pub fn load(layout: &Layout) -> Prefs {
    match std::fs::read_to_string(path(layout)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("settings.json unreadable ({e}), using defaults");
            Prefs::default()
        }),
        Err(_) => Prefs::default(),
    }
}

pub fn save(layout: &Layout, prefs: &Prefs) -> Result<Prefs> {
    let mut p = prefs.clone();
    p.voice_volume = p.voice_volume.clamp(0.0, 1.0);
    p.wake_words = p
        .wake_words
        .iter()
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();
    if p.wake_words.is_empty() {
        p.wake_words = Prefs::default().wake_words;
    }
    if p.voice_pack.trim().is_empty() || p.voice_pack.contains(['/', '\\', '.']) {
        p.voice_pack = Prefs::default().voice_pack;
    }
    p.digest.times = p
        .digest
        .times
        .iter()
        .filter_map(|t| {
            let t = t.trim();
            let (h, m) = t.split_once(':')?;
            let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
            (h < 24 && m < 60).then(|| format!("{h:02}:{m:02}"))
        })
        .collect();
    p.digest.times.sort();
    p.digest.times.dedup();
    let path = path(layout);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&p)?)?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip_and_sanitise() {
        let root = std::env::temp_dir().join(format!("hermes-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        assert_eq!(load(&layout), Prefs::default());
        let p = Prefs {
            voice_volume: 1.7,
            silent: true,
            wake_words: vec![" Джарвис ".into(), "".into()],
            wake_required: false,
            voice_pack: "../x".into(),
            digest: Digest {
                enabled: true,
                times: vec!["9:5".into(), "bad".into(), "17:00".into(), "09:05".into()],
                speak: true,
                telegram: false,
            },
            claude_project: "hermes-assistant".into(),
        };
        let saved = save(&layout, &p).unwrap();
        assert_eq!(saved.voice_volume, 1.0);
        assert_eq!(saved.wake_words, vec!["джарвис"]);
        assert_eq!(saved.voice_pack, "Jarvis");
        assert_eq!(saved.digest.times, vec!["09:05", "17:00"]);
        assert_eq!(load(&layout), saved);
        // Unknown/missing fields fall back to defaults.
        std::fs::write(path(&layout), r#"{"silent": true}"#).unwrap();
        let l = load(&layout);
        assert!(l.silent);
        assert_eq!(l.voice_volume, 0.8);
        let _ = std::fs::remove_dir_all(&root);
    }
}
