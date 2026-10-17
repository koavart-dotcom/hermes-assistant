//! Quick commands: spoken (or typed) phrases matched against a local pack and
//! executed in milliseconds without the model — window management, launching
//! apps and sites, volume, media keys, lock/sleep/shutdown — with a voice
//! clip as the reaction. Modelled on the command tree of Luxify Assistant
//! (phrases + optional words + a sequence of primitives); the pack itself is
//! ours (`hermes-quick-commands/default.ru.json`).
//!
//! Safety: commands flagged `confirm` (sleep, restart, shutdown) run only
//! after the UI confirmed; everything is logged to `quick_commands.log`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CoreError, Layout, Result};

// ------------------------------------------------------------------ pack --

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuickCommand {
    pub id: String,
    pub name: String,
    pub phrases: Vec<String>,
    #[serde(default)]
    pub confirm: bool,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuickPack {
    pub version: u32,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub wake_words: Vec<String>,
    #[serde(default)]
    pub optional_words: Vec<String>,
    pub commands: Vec<QuickCommand>,
}

impl QuickPack {
    pub fn load(path: &Path) -> Result<QuickPack> {
        let text = std::fs::read_to_string(path)?;
        let pack: QuickPack = serde_json::from_str(&text)?;
        for c in &pack.commands {
            for a in &c.actions {
                parse_action(a).map_err(|e| {
                    CoreError::Other(format!("quick pack: command `{}`: {e}", c.id))
                })?;
            }
        }
        Ok(pack)
    }

    /// Merge a user pack on top: commands with the same `id` replace, others append.
    pub fn merge(&mut self, user: QuickPack) {
        for c in user.commands {
            if let Some(slot) = self.commands.iter_mut().find(|x| x.id == c.id) {
                *slot = c;
            } else {
                self.commands.push(c);
            }
        }
        self.wake_words.extend(user.wake_words);
        self.optional_words.extend(user.optional_words);
    }
}

// --------------------------------------------------------------- matching --

/// A matched command, as shown to the UI.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct QuickMatch {
    pub id: String,
    pub name: String,
    pub score: f64,
    pub needs_confirm: bool,
    pub stops_speech: bool,
    /// Free part of a parametrised phrase (`найди в ютубе {query}` → the rest).
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct QuickCommandInfo {
    pub id: String,
    pub name: String,
    pub phrases: Vec<String>,
    pub confirm: bool,
}

/// Lowercase, `ё`→`е`, punctuation out, single spaces.
pub fn normalize(text: &str) -> String {
    let lowered = text.to_lowercase().replace('ё', "е");
    let mut out = String::with_capacity(lowered.len());
    for ch in lowered.chars() {
        if ch.is_alphanumeric() || ch == '+' {
            out.push(ch);
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_words(text: &str, words: &[String]) -> String {
    text.split_whitespace()
        .filter(|w| !words.iter().any(|o| o == w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Levenshtein ratio in `0..=1` (1 = identical), on chars.
pub fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let dist = prev[b.len()];
    1.0 - dist as f64 / a.len().max(b.len()) as f64
}

pub const MATCH_THRESHOLD: f64 = 0.82;
/// Placeholder in phrases and actions of parametrised commands.
pub const QUERY: &str = "{query}";

/// Walk the raw words, skipping wake/filler words, until their normalised
/// tokens cover `prefix`; return the untouched remainder (trailing
/// punctuation trimmed) or `None` when the raw text does not start that way.
fn raw_tail_after(raw: &str, prefix: &str, optional: &[String], wake: &[String]) -> Option<String> {
    let want: Vec<&str> = prefix.split(' ').collect();
    let mut got = 0usize;
    let words: Vec<&str> = raw.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() && got < want.len() {
        let n = normalize(words[i]);
        i += 1;
        if n.is_empty() || optional.contains(&n) || wake.contains(&n) {
            continue;
        }
        for tok in n.split(' ') {
            if got < want.len() && want[got] == tok {
                got += 1;
            } else {
                return None;
            }
        }
    }
    if got < want.len() {
        return None;
    }
    // Filler words right after the prefix ("загугли, пожалуйста, курс") are not the query.
    while i < words.len() {
        let n = normalize(words[i]);
        if n.is_empty() || optional.contains(&n) {
            i += 1;
        } else {
            break;
        }
    }
    let tail = words[i..].join(" ");
    let q = tail
        .trim_end_matches(['.', '!', '?', ',', ';', ':'])
        .trim()
        .to_string();
    (!q.is_empty()).then_some(q)
}

/// Substitute `{query}` into an action string: percent-encoded inside
/// `url:`, verbatim elsewhere. Actions without the placeholder are returned as is.
pub fn fill_action(action: &str, query: Option<&str>) -> String {
    if !action.contains(QUERY) {
        return action.to_string();
    }
    let q = query.unwrap_or("");
    if action.starts_with("ticket_draft:") || action.starts_with("ticket_open:") {
        let digits: String = q.chars().filter(|c| c.is_ascii_digit()).collect();
        return action.replace(QUERY, &digits);
    }
    if let Some(rest) = action.strip_prefix("url:") {
        // `url:https://{query}` → keep a bare host usable; everything else is encoded.
        let value = if rest.trim_start().starts_with(QUERY)
            || rest.trim_start().starts_with("https://{query}")
            || rest.trim_start().starts_with("http://{query}")
        {
            q.trim()
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .to_string()
        } else {
            crate::freescout::url_encode(q.trim())
        };
        format!("url:{}", rest.replace(QUERY, &value))
    } else {
        action.replace(QUERY, q.trim())
    }
}

impl QuickPack {
    /// Text after normalization, without wake words at the start and
    /// without optional filler words.
    pub fn prepare(&self, text: &str) -> String {
        let mut t = normalize(text);
        let wake: Vec<String> = self.wake_words.iter().map(|w| normalize(w)).collect();
        let mut changed = true;
        while changed {
            changed = false;
            for w in &wake {
                if let Some(rest) = t.strip_prefix(w.as_str()) {
                    let rest = rest.trim_start();
                    if rest.len() != t.len() && (rest.is_empty() || t.as_bytes()[w.len()] == b' ') {
                        t = rest.to_string();
                        changed = true;
                    }
                }
            }
        }
        let optional: Vec<String> = self.optional_words.iter().map(|w| normalize(w)).collect();
        strip_words(&t, &optional)
    }

    /// Best command for `text`, or `None` when nothing is close enough.
    pub fn find(&self, text: &str) -> Option<QuickMatch> {
        let query = self.prepare(text);
        if query.is_empty() {
            return None;
        }
        let optional: Vec<String> = self.optional_words.iter().map(|w| normalize(w)).collect();
        let mut best: Option<(f64, &QuickCommand)> = None;
        for c in &self.commands {
            for p in &c.phrases {
                let phrase = strip_words(&normalize(p), &optional);
                if phrase.is_empty() {
                    continue;
                }
                let score = if phrase == query {
                    1.0
                } else {
                    // Whole phrase inside a longer utterance ("открой мне браузер
                    // на втором мониторе" contains "открой браузер"): accept
                    // when at most 3 extra words surround it.
                    let pw: Vec<&str> = phrase.split(' ').collect();
                    let qw: Vec<&str> = query.split(' ').collect();
                    let contained = pw.iter().all(|w| qw.contains(w));
                    let extra = qw.len().saturating_sub(pw.len());
                    if contained && extra <= 3 {
                        0.9 - 0.02 * extra as f64
                    } else {
                        similarity(&phrase, &query)
                    }
                };
                if score >= MATCH_THRESHOLD && best.is_none_or(|(s, _)| score > s) {
                    best = Some((score, c));
                }
            }
        }
        // An explicit parametrised phrase ("открой сайт …") beats a fuzzy or
        // contained fixed one; only an exact fixed phrase beats it.
        let param = self.find_parametrised(text, &query, &optional);
        match (best, param) {
            (Some((score, c)), p) if score >= 1.0 || p.is_none() => Some(QuickMatch {
                id: c.id.clone(),
                name: c.name.clone(),
                score,
                needs_confirm: c.confirm,
                stops_speech: c.actions.iter().any(|a| a == "stop_speech"),
                query: None,
            }),
            (_, p) => p,
        }
    }

    /// Phrases with `{query}`: the prepared text must start with the phrase
    /// prefix; what follows is the query, taken from the raw text so that
    /// dots and case survive (`открой сайт github.com`).
    fn find_parametrised(
        &self,
        raw: &str,
        prepared: &str,
        optional: &[String],
    ) -> Option<QuickMatch> {
        let wake: Vec<String> = self.wake_words.iter().map(|w| normalize(w)).collect();
        let mut best: Option<(usize, &QuickCommand, String)> = None;
        for c in &self.commands {
            for p in c.phrases.iter().filter(|p| p.contains(QUERY)) {
                let prefix = strip_words(&normalize(&p.replace(QUERY, "")), optional);
                if prefix.is_empty() {
                    continue;
                }
                let Some(rest) = prepared.strip_prefix(prefix.as_str()) else {
                    continue;
                };
                if !rest.starts_with(' ') || rest.trim().is_empty() {
                    continue;
                }
                let Some(query) = raw_tail_after(raw, &prefix, optional, &wake) else {
                    continue;
                };
                // Longest prefix wins ("найди в ютубе" over "найди").
                let plen = prefix.len();
                if best.as_ref().is_none_or(|(l, _, _)| plen > *l) {
                    best = Some((plen, c, query));
                }
            }
        }
        best.map(|(_, c, query)| QuickMatch {
            id: c.id.clone(),
            name: c.name.clone(),
            score: 0.95,
            needs_confirm: c.confirm,
            stops_speech: false,
            query: Some(query),
        })
    }

    pub fn get(&self, id: &str) -> Option<&QuickCommand> {
        self.commands.iter().find(|c| c.id == id)
    }

    pub fn infos(&self) -> Vec<QuickCommandInfo> {
        self.commands
            .iter()
            .map(|c| QuickCommandInfo {
                id: c.id.clone(),
                name: c.name.clone(),
                phrases: c.phrases.clone(),
                confirm: c.confirm,
            })
            .collect()
    }
}

// ---------------------------------------------------------------- actions --

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Play `voice-pack/<voice>/<path>.wav`.
    Say(String),
    /// Key chord like `win+d`, `ctrl+shift+esc`, `mute`, `media_next`.
    Key(Vec<String>),
    VolumeDelta(i32),
    VolumeSet(u8),
    PauseMs(u64),
    MonitorOff,
    MonitorOn,
    /// Start Menu shortcut name(s), `|`-separated alternatives.
    App(Vec<String>),
    Url(String),
    Lock,
    Sleep,
    Restart,
    Shutdown,
    StopSpeech,
    /// Ask the shell UI to open something (`tickets`, `hud`, `hud_off`); no-op in the executor.
    Ui(String),
    /// Search YouTube and open the first video.
    YtPlay(String),
    /// Open the N-th (1-based) video of the last search query.
    YtPick(usize),
    /// Open the N-th (1-based) web result of the last search query.
    WebPick(usize),
    /// Read the current browser tab (address bar → clipboard → fetch) and hand
    /// the text to the model.
    ReadPage,
    /// Type text into the active window (Unicode SendInput).
    TypeText(String),
    /// Draft a reply for ticket N: the transcript goes to the model as a handoff.
    TicketDraft(u64),
    /// Open ticket N in the browser.
    TicketOpen(u64),
    /// Hand a task to Claude Code through the Hermes MCP bridge (`claude_task`).
    ClaudeTask(String),
}

pub const KEY_NAMES: &[&str] = &[
    "win",
    "ctrl",
    "alt",
    "shift",
    "enter",
    "esc",
    "tab",
    "space",
    "left",
    "right",
    "up",
    "down",
    "home",
    "end",
    "pageup",
    "pagedown",
    "delete",
    "backspace",
    "printscreen",
    "mute",
    "volume_up",
    "volume_down",
    "media_play_pause",
    "media_next",
    "media_prev",
    ".",
    "+",
    "-",
    "=",
];

pub fn parse_action(s: &str) -> std::result::Result<Action, String> {
    let (kind, arg) = s.split_once(':').unwrap_or((s, ""));
    let arg = arg.trim();
    Ok(match kind.trim() {
        "say" => Action::Say(arg.to_string()),
        "key" => {
            let parts: Vec<String> = arg.split('+').map(|p| p.trim().to_lowercase()).collect();
            if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
                return Err(format!("bad key chord `{arg}`"));
            }
            for p in &parts {
                let known = KEY_NAMES.contains(&p.as_str())
                    || (p.len() == 1 && p.chars().all(|c| c.is_ascii_alphanumeric()))
                    || (p.starts_with('f')
                        && p[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)));
                if !known {
                    return Err(format!("unknown key `{p}` in `{arg}`"));
                }
            }
            Action::Key(parts)
        }
        "volume" => {
            if let Some(v) = arg.strip_prefix('=') {
                Action::VolumeSet(v.parse().map_err(|_| format!("bad volume `{arg}`"))?)
            } else {
                Action::VolumeDelta(arg.parse().map_err(|_| format!("bad volume `{arg}`"))?)
            }
        }
        "pause" => Action::PauseMs(arg.parse().map_err(|_| format!("bad pause `{arg}`"))?),
        "monitor" => match arg {
            "off" => Action::MonitorOff,
            "on" => Action::MonitorOn,
            _ => return Err(format!("monitor: `{arg}`")),
        },
        "app" => Action::App(arg.split('|').map(|s| s.trim().to_string()).collect()),
        "url" => Action::Url(arg.to_string()),
        "lock" => Action::Lock,
        "sleep" => Action::Sleep,
        "restart" => Action::Restart,
        "shutdown" => Action::Shutdown,
        "stop_speech" => Action::StopSpeech,
        "ui" => {
            if !matches!(
                arg,
                "tickets" | "hud" | "hud_off" | "digest" | "ticket_send" | "quiet_on" | "quiet_off"
            ) {
                return Err(format!("unknown ui target `{arg}`"));
            }
            Action::Ui(arg.to_string())
        }
        "yt_play" => {
            if arg.is_empty() {
                return Err("yt_play needs a query".into());
            }
            Action::YtPlay(arg.to_string())
        }
        "yt_pick" => Action::YtPick(parse_index(arg)?),
        "web_pick" => Action::WebPick(parse_index(arg)?),
        "read_page" => Action::ReadPage,
        "type" => Action::TypeText(arg.to_string()),
        "ticket_draft" => Action::TicketDraft(parse_ticket(arg)?),
        "ticket_open" => Action::TicketOpen(parse_ticket(arg)?),
        "claude" => {
            if arg.is_empty() {
                return Err("claude needs a task".into());
            }
            Action::ClaudeTask(arg.to_string())
        }
        other => return Err(format!("unknown action `{other}`")),
    })
}

/// Ticket number from speech: digits only, "4210", "номер 4210", "4 2 1 0", "{query}" placeholder allowed.
fn parse_ticket(arg: &str) -> std::result::Result<u64, String> {
    if arg.contains(QUERY) {
        return Ok(0); // filled in later from the spoken query
    }
    let digits: String = arg.chars().filter(|c| c.is_ascii_digit()).collect();
    digits
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("bad ticket number `{arg}`"))
}

fn parse_index(arg: &str) -> std::result::Result<usize, String> {
    match arg.parse::<usize>() {
        Ok(n) if (1..=20).contains(&n) => Ok(n),
        _ => Err(format!("bad index `{arg}` (1..20)")),
    }
}

pub fn is_dangerous(a: &Action) -> bool {
    matches!(a, Action::Sleep | Action::Restart | Action::Shutdown)
}

/// Open `url` in the default browser (used by the UI for ticket links).
pub async fn open_url(url: &str) -> Result<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(CoreError::Other(format!(
            "refusing to open `{url}`: not http(s)"
        )));
    }
    platform::open_url(url).await
}

// --------------------------------------------------------------- executor --

pub fn voice_pack_dir(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("voice-pack")
}

pub fn log_path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("quick_commands.log")
}

/// Play a reaction clip outside quick commands (the acknowledgement before a
/// model answer, see `Supervisor::voice_ack`).
pub async fn play_clip(path: &Path, volume: f64) -> Result<()> {
    platform::play_wav(path, volume).await
}

/// Resolve `say:` to a file: `<voice-pack>/<voice>/<path>.wav`, trying the
/// plain clip and the ", сэр" variant.
pub fn clip_path(layout: &Layout, voice: &str, clip: &str) -> Option<PathBuf> {
    let base = voice_pack_dir(layout).join(voice);
    let candidates = [
        base.join(format!("{clip}.wav")),
        base.join(format!("{clip}, сэр.wav")),
    ];
    candidates.into_iter().find(|p| p.exists())
}

/// Copy `*.wav` from `src` (e.g. Luxify's `assets\Voices\Jarvis`) into
/// `<voice-pack>/<name>/`, keeping subfolders. Returns the number of clips.
pub fn import_voice_pack(layout: &Layout, src: &Path, name: &str) -> Result<usize> {
    if !src.is_dir() {
        return Err(CoreError::Other(format!(
            "{} is not a directory",
            src.display()
        )));
    }
    let dest = voice_pack_dir(layout).join(name);
    let mut n = 0;
    fn walk(src: &Path, dest: &Path, n: &mut usize) -> Result<()> {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(&path, &dest.join(entry.file_name()), n)?;
            } else if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
            {
                std::fs::copy(&path, dest.join(entry.file_name()))?;
                *n += 1;
            }
        }
        Ok(())
    }
    walk(src, &dest, &mut n)?;
    Ok(n)
}

/// Executes actions on this machine. Only `windows` does anything; other
/// targets log and return (so the crate tests everywhere).
pub struct Executor {
    pub layout: Layout,
    pub voice: String,
    /// Last search query (set by the supervisor for `web.*` commands) so
    /// "включи первое видео" / "открой вторую ссылку" know what to pick.
    pub last_query: Option<String>,
    /// Reaction clip volume 0..1 and the silent switch (from preferences).
    pub volume: f64,
    pub silent: bool,
    /// Project for `claude:` tasks (from preferences).
    pub claude_project: String,
}

/// What the shell should do after the actions ran.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Outcome {
    /// Text to send to the model as a chat turn (e.g. a page to summarise).
    pub handoff: Option<String>,
    /// Something to say back (quick, no model).
    pub spoken: Option<String>,
    /// Conversation id a drafted reply belongs to (set by `ticket_draft`).
    pub ticket: Option<u64>,
}

impl Executor {
    pub fn new(layout: Layout, voice: impl Into<String>) -> Executor {
        Executor {
            layout,
            voice: voice.into(),
            last_query: None,
            volume: 1.0,
            silent: false,
            claude_project: "hermes-assistant".into(),
        }
    }

    pub async fn run(&self, actions: &[Action]) -> Result<Outcome> {
        let mut out = Outcome::default();
        for a in actions {
            match a {
                Action::YtPlay(q) => {
                    let hits = crate::web::youtube_search(q).await?;
                    platform::open_url(&hits[0].url).await?;
                }
                Action::YtPick(n) => {
                    let q = self.last_query.as_deref().ok_or_else(|| {
                        CoreError::Other("сначала скажите, что искать на YouTube".into())
                    })?;
                    let hits = crate::web::youtube_search(q).await?;
                    let hit = hits.get(n - 1).ok_or_else(|| {
                        CoreError::Other(format!("на YouTube нашлось меньше {n} видео"))
                    })?;
                    platform::open_url(&hit.url).await?;
                }
                Action::WebPick(n) => {
                    let q = self
                        .last_query
                        .as_deref()
                        .ok_or_else(|| CoreError::Other("сначала скажите, что искать".into()))?;
                    let hits = crate::web::web_search(q).await?;
                    let hit = hits.get(n - 1).ok_or_else(|| {
                        CoreError::Other(format!("нашлось меньше {n} результатов"))
                    })?;
                    out.spoken = Some(hit.title.clone());
                    platform::open_url(&hit.url).await?;
                }
                Action::ReadPage => {
                    let url = platform::current_url().await?;
                    let text = crate::web::page_text(&url, 6000).await?;
                    out.handoff = Some(format!(
                        "Кратко перескажи по-русски, о чём эта страница ({url}), 3–5 предложений, без вступления:\n\n{text}"
                    ));
                }
                Action::TypeText(t) => platform::type_text(t).await?,
                Action::TicketDraft(n) => {
                    let cfg = crate::freescout::config(&self.layout)?.ok_or_else(|| {
                        CoreError::Other(
                            "FreeScout не настроен (Настройки → Интеграции и ключи)".into(),
                        )
                    })?;
                    let conv = crate::freescout::Client::new(cfg)
                        .conversation_text(*n)
                        .await?;
                    out.spoken = Some(format!(
                        "Готовлю ответ на заявку {}: {}",
                        conv.ticket.number, conv.ticket.subject
                    ));
                    out.handoff = Some(crate::freescout::draft_prompt(&conv));
                    out.ticket = Some(conv.id);
                }
                Action::ClaudeTask(task) => {
                    out.spoken = Some("Передаю Клоду.".into());
                    out.handoff = Some(format!(
                        "Используй инструмент claude_task (MCP-сервер claude) с параметрами project=\"{}\" и task=\"{}\". \
Дождись результата и перескажи его по-русски кратко: что сделано и что вернул Клод.",
                        self.claude_project,
                        task.replace('"', "'")
                    ));
                }
                Action::TicketOpen(n) => {
                    let cfg = crate::freescout::config(&self.layout)?
                        .ok_or_else(|| CoreError::Other("FreeScout не настроен".into()))?;
                    let client = crate::freescout::Client::new(cfg.clone());
                    let id = client.find_by_number(*n).await?;
                    platform::open_url(&format!("{}/conversation/{id}", cfg.url)).await?;
                }
                Action::Say(clip) => {
                    if self.silent {
                        continue;
                    }
                    if let Some(path) = clip_path(&self.layout, &self.voice, clip) {
                        platform::play_wav(&path, self.volume).await?;
                    } else {
                        log::debug!(target: "quick", "no clip for `{clip}` (voice {})", self.voice);
                    }
                }
                Action::Key(chord) => platform::press(chord).await?,
                Action::VolumeDelta(d) => platform::volume_delta(*d).await?,
                Action::VolumeSet(v) => platform::volume_set(*v).await?,
                Action::PauseMs(ms) => tokio::time::sleep(Duration::from_millis(*ms)).await,
                Action::MonitorOff => platform::monitor(false).await?,
                Action::MonitorOn => platform::monitor(true).await?,
                Action::App(names) => platform::launch_app(names).await?,
                Action::Url(url) => platform::open_url(url).await?,
                Action::Lock => platform::lock().await?,
                Action::Sleep => platform::power("sleep").await?,
                Action::Restart => platform::power("restart").await?,
                Action::Shutdown => platform::power("shutdown").await?,
                Action::StopSpeech | Action::Ui(_) => {}
            }
        }
        Ok(out)
    }
}

/// Windows implementation through PowerShell (hidden) and SendInput. Kept
/// in one place so it can be swapped for direct WinAPI calls later.
#[cfg(windows)]
mod platform {
    use std::path::Path;
    use std::process::Stdio;
    use std::time::Duration;

    use tokio::process::Command;

    use crate::cli::hide_console;
    use crate::{CoreError, Result};

    async fn ps(script: &str) -> Result<()> {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut cmd);
        let status = cmd.status().await?;
        if !status.success() {
            return Err(CoreError::ProcessFailed(
                "powershell (quick)".into(),
                status.to_string(),
            ));
        }
        Ok(())
    }

    fn q(s: &str) -> String {
        s.replace('\'', "''")
    }

    /// Play a clip; below full volume a WPF MediaPlayer is used (SoundPlayer
    /// has no volume control).
    pub async fn play_wav(path: &Path, volume: f64) -> Result<()> {
        let p = q(&path.display().to_string());
        if volume >= 0.99 {
            return ps(&format!(
                "$p = New-Object System.Media.SoundPlayer '{p}'; $p.PlaySync()"
            ))
            .await;
        }
        let v = volume.clamp(0.0, 1.0);
        ps(&format!(
            "Add-Type -AssemblyName PresentationCore; $m = New-Object System.Windows.Media.MediaPlayer; \
             $m.Open([Uri]'{p}'); $m.Volume = {v}; $i = 0; \
             while (-not $m.NaturalDuration.HasTimeSpan -and $i -lt 100) {{ Start-Sleep -Milliseconds 20; $i++ }}; \
             $m.Play(); $ms = if ($m.NaturalDuration.HasTimeSpan) {{ $m.NaturalDuration.TimeSpan.TotalMilliseconds }} else {{ 3000 }}; \
             Start-Sleep -Milliseconds ([int]$ms + 150); $m.Close()"
        ))
        .await
    }

    fn vk(name: &str) -> Option<u16> {
        Some(match name {
            "win" => 0x5B,
            "ctrl" => 0x11,
            "alt" => 0x12,
            "shift" => 0x10,
            "enter" => 0x0D,
            "esc" => 0x1B,
            "tab" => 0x09,
            "space" => 0x20,
            "left" => 0x25,
            "up" => 0x26,
            "right" => 0x27,
            "down" => 0x28,
            "home" => 0x24,
            "end" => 0x23,
            "pageup" => 0x21,
            "pagedown" => 0x22,
            "delete" => 0x2E,
            "backspace" => 0x08,
            "printscreen" => 0x2C,
            "mute" => 0xAD,
            "volume_down" => 0xAE,
            "volume_up" => 0xAF,
            "media_next" => 0xB0,
            "media_prev" => 0xB1,
            "media_play_pause" => 0xB3,
            "." => 0xBE,
            "+" | "=" => 0xBB,
            "-" => 0xBD,
            s if s.len() == 1 => {
                let c = s.as_bytes()[0].to_ascii_uppercase();
                if c.is_ascii_alphanumeric() {
                    c as u16
                } else {
                    return None;
                }
            }
            s if s.starts_with('f') => 0x70 + s[1..].parse::<u16>().ok()? - 1,
            _ => return None,
        })
    }

    pub async fn press(chord: &[String]) -> Result<()> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
        };
        let codes: Vec<u16> = chord
            .iter()
            .map(|k| vk(k).ok_or_else(|| CoreError::Other(format!("unknown key `{k}`"))))
            .collect::<Result<_>>()?;
        let mk = |code: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: code,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut inputs: Vec<INPUT> = codes.iter().map(|&c| mk(c, false)).collect();
        inputs.extend(codes.iter().rev().map(|&c| mk(c, true)));
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != inputs.len() {
            return Err(CoreError::Other("SendInput was blocked".into()));
        }
        Ok(())
    }

    /// Type Unicode text into the focused window.
    pub async fn type_text(text: &str) -> Result<()> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
            SendInput,
        };
        let mk = |unit: u16, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 4);
        for unit in text.encode_utf16() {
            inputs.push(mk(unit, false));
            inputs.push(mk(unit, true));
        }
        if inputs.is_empty() {
            return Ok(());
        }
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent as usize != inputs.len() {
            return Err(CoreError::Other("SendInput (text) was blocked".into()));
        }
        Ok(())
    }

    /// URL of the active browser tab: focus the address bar, copy it, restore
    /// the previous clipboard. Works in Chrome/Edge/Firefox.
    pub async fn current_url() -> Result<String> {
        let before = ps_out("Get-Clipboard -Raw").await.unwrap_or_default();
        press(&["ctrl".into(), "l".into()]).await?;
        tokio::time::sleep(Duration::from_millis(150)).await;
        press(&["ctrl".into(), "c".into()]).await?;
        tokio::time::sleep(Duration::from_millis(150)).await;
        press(&["esc".into()]).await?;
        let url = ps_out("Get-Clipboard -Raw").await?.trim().to_string();
        if !before.trim().is_empty() && before != url {
            let _ = ps(&format!("Set-Clipboard -Value '{}'", q(before.trim_end()))).await;
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(CoreError::Other(format!(
                "в адресной строке не адрес страницы: «{}»",
                url.chars().take(60).collect::<String>()
            )));
        }
        Ok(url)
    }

    async fn ps_out(script: &str) -> Result<String> {
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut cmd);
        let out = cmd.output().await?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub async fn volume_delta(delta: i32) -> Result<()> {
        let key = if delta >= 0 {
            "volume_up"
        } else {
            "volume_down"
        };
        // Every media key press moves the master volume by 2 %.
        for _ in 0..(delta.unsigned_abs() / 2).max(1) {
            press(&[key.to_string()]).await?;
        }
        Ok(())
    }

    pub async fn volume_set(percent: u8) -> Result<()> {
        for _ in 0..50 {
            press(&["volume_down".to_string()]).await?;
        }
        for _ in 0..(u32::from(percent.min(100)) / 2) {
            press(&["volume_up".to_string()]).await?;
        }
        Ok(())
    }

    pub async fn monitor(on: bool) -> Result<()> {
        if on {
            // A tiny mouse move wakes the display.
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput,
            };
            let mut inputs = [INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 1,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_MOVE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            }];
            unsafe {
                SendInput(1, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
            }
            Ok(())
        } else {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                HWND_BROADCAST, SC_MONITORPOWER, SendMessageW, WM_SYSCOMMAND,
            };
            unsafe {
                SendMessageW(HWND_BROADCAST, WM_SYSCOMMAND, SC_MONITORPOWER as usize, 2);
            }
            Ok(())
        }
    }

    pub async fn launch_app(names: &[String]) -> Result<()> {
        let list = names
            .iter()
            .map(|n| format!("'{}'", q(n)))
            .collect::<Vec<_>>()
            .join(",");
        // Fast path first: a program name Windows resolves itself (PATH or
        // the App Paths registry — chrome, code, notepad, calc, ...), which
        // is instant. Only then search Start Menu shortcuts (slow, ~1-3 s).
        // 1) Already running with a window? Restore it (if minimized) and bring
        //    it to the front instead of opening another instance.
        // 2) Else start by program name, 3) else via a Start Menu shortcut.
        let script = format!(
            "$names = @({list}); \
             Add-Type -Namespace HA -Name Win -MemberDefinition '[DllImport(\"user32.dll\")] public static extern bool ShowWindowAsync(IntPtr h, int c); [DllImport(\"user32.dll\")] public static extern bool SetForegroundWindow(IntPtr h); [DllImport(\"user32.dll\")] public static extern bool IsIconic(IntPtr h);'; \
             foreach ($n in $names) {{ $p = Get-Process -Name $n -ErrorAction SilentlyContinue | Where-Object {{ $_.MainWindowHandle -ne 0 }} | Select-Object -First 1; \
               if ($p) {{ $h = $p.MainWindowHandle; if ([HA.Win]::IsIconic($h)) {{ [HA.Win]::ShowWindowAsync($h, 9) | Out-Null }}; [HA.Win]::SetForegroundWindow($h) | Out-Null; exit 0 }} }}; \
             foreach ($n in $names) {{ try {{ Start-Process -FilePath $n -ErrorAction Stop; exit 0 }} catch {{}} }}; \
             $dirs = @(\"$env:ProgramData\\Microsoft\\Windows\\Start Menu\\Programs\", \"$env:APPDATA\\Microsoft\\Windows\\Start Menu\\Programs\"); \
             $lnk = Get-ChildItem $dirs -Recurse -Filter *.lnk -ErrorAction SilentlyContinue | Where-Object {{ $b = $_.BaseName; $names | Where-Object {{ $b -like \"*$_*\" }} }} | Select-Object -First 1; \
             if ($lnk) {{ Start-Process -FilePath $lnk.FullName }} else {{ exit 1 }}"
        );
        ps(&script).await
    }

    pub async fn open_url(url: &str) -> Result<()> {
        ps(&format!("Start-Process '{}'", q(url))).await
    }

    pub async fn lock() -> Result<()> {
        use windows_sys::Win32::System::Shutdown::LockWorkStation;
        if unsafe { LockWorkStation() } == 0 {
            return Err(CoreError::Other("LockWorkStation failed".into()));
        }
        Ok(())
    }

    pub async fn power(what: &str) -> Result<()> {
        match what {
            "sleep" => ps("rundll32.exe powrprof.dll,SetSuspendState 0,1,0").await,
            "restart" => ps("shutdown.exe /r /t 5").await,
            "shutdown" => ps("shutdown.exe /s /t 5").await,
            _ => Err(CoreError::Other(format!("power: {what}"))),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    use crate::Result;

    fn skip(what: &str) -> Result<()> {
        log::info!(target: "quick", "skipped on this OS: {what}");
        Ok(())
    }
    pub async fn play_wav(p: &Path, _volume: f64) -> Result<()> {
        skip(&format!("play {}", p.display()))
    }
    pub async fn press(chord: &[String]) -> Result<()> {
        skip(&format!("key {}", chord.join("+")))
    }
    pub async fn type_text(t: &str) -> Result<()> {
        skip(&format!("type {t}"))
    }
    pub async fn current_url() -> Result<String> {
        Err(crate::CoreError::Other(
            "reading the browser address bar works on Windows only".into(),
        ))
    }
    pub async fn volume_delta(d: i32) -> Result<()> {
        skip(&format!("volume {d:+}"))
    }
    pub async fn volume_set(p: u8) -> Result<()> {
        skip(&format!("volume ={p}"))
    }
    pub async fn monitor(on: bool) -> Result<()> {
        skip(&format!("monitor {on}"))
    }
    pub async fn launch_app(n: &[String]) -> Result<()> {
        skip(&format!("app {}", n.join("|")))
    }
    pub async fn open_url(u: &str) -> Result<()> {
        skip(&format!("url {u}"))
    }
    pub async fn lock() -> Result<()> {
        skip("lock")
    }
    pub async fn power(w: &str) -> Result<()> {
        skip(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> QuickPack {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../hermes-quick-commands/default.ru.json");
        QuickPack::load(&path).unwrap()
    }

    #[test]
    fn default_pack_loads_and_all_actions_parse() {
        let p = pack();
        assert!(p.commands.len() >= 50);
        let ids: std::collections::HashSet<_> = p.commands.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids.len(), p.commands.len(), "duplicate ids");
        assert!(p.get("pwr.shutdown").unwrap().confirm);
        assert!(!p.get("win.desktop").unwrap().confirm);
    }

    #[test]
    fn matches_exact_fuzzy_and_with_wake_and_filler_words() {
        let p = pack();
        assert_eq!(p.find("Сверни все окна").unwrap().id, "win.desktop");
        assert_eq!(
            p.find("Джарвис, сверни, пожалуйста, все окна!").unwrap().id,
            "win.desktop"
        );
        assert_eq!(p.find("гермес открой телеграм").unwrap().id, "app.telegram");
        // Phrase inside a longer utterance (Whisper adds filler and context).
        assert_eq!(
            p.find("джарвис открой мне браузер на втором экране")
                .unwrap()
                .id,
            "app.chrome"
        );
        assert_eq!(p.find("открой браузер").unwrap().id, "app.chrome");
        // Whisper-style small slip.
        assert_eq!(p.find("сделай громчи").unwrap().id, "snd.up");
        let m = p.find("выключи компьютер").unwrap();
        assert_eq!(m.id, "pwr.shutdown");
        assert!(m.needs_confirm);
        assert!(p.find("стоп").unwrap().stops_speech);
    }

    #[test]
    fn parametrised_phrases_take_the_tail_as_query() {
        let p = pack();
        let m = p.find("Джарвис, найди в ютубе котиков на пианино").unwrap();
        assert_eq!(m.id, "web.youtube_search");
        assert_eq!(m.query.as_deref(), Some("котиков на пианино"));
        let m = p.find("открой сайт github.com").unwrap();
        assert_eq!(m.id, "web.open_site");
        assert_eq!(m.query.as_deref(), Some("github.com"));
        let m = p.find("загугли, пожалуйста, курс доллара.").unwrap();
        assert_eq!(m.id, "web.google");
        assert_eq!(m.query.as_deref(), Some("курс доллара"));
        assert_eq!(
            p.find("запусти программу paint").unwrap().query.as_deref(),
            Some("paint")
        );
        // Fixed phrases still win and carry no query.
        assert!(p.find("открой браузер").unwrap().query.is_none());
        // "включи видео" is the open video, not a YouTube search for «видео».
        let m = p.find("джарвис включи видео").unwrap();
        assert_eq!(m.id, "media.video_play");
        assert!(m.query.is_none());
        assert_eq!(p.find("включи котиков").unwrap().id, "web.play");
        // A bare prefix without a query is not a match.
        assert!(
            p.find("найди в ютубе").is_none() || p.find("найди в ютубе").unwrap().query.is_none()
        );
    }

    #[test]
    fn parses_browser_actions() {
        assert_eq!(
            parse_action("yt_play:котики").unwrap(),
            Action::YtPlay("котики".into())
        );
        assert_eq!(parse_action("yt_pick:2").unwrap(), Action::YtPick(2));
        assert_eq!(parse_action("web_pick:1").unwrap(), Action::WebPick(1));
        assert_eq!(parse_action("read_page").unwrap(), Action::ReadPage);
        assert_eq!(
            parse_action("type:привет").unwrap(),
            Action::TypeText("привет".into())
        );
        assert!(parse_action("yt_pick:0").is_err());
        assert!(parse_action("yt_play:").is_err());
    }

    #[test]
    fn fills_query_into_actions() {
        assert_eq!(
            fill_action(
                "url:https://www.youtube.com/results?search_query={query}",
                Some("кот и пёс")
            ),
            "url:https://www.youtube.com/results?search_query=%D0%BA%D0%BE%D1%82%20%D0%B8%20%D0%BF%D1%91%D1%81"
        );
        assert_eq!(
            fill_action("url:https://{query}", Some("https://github.com")),
            "url:https://github.com"
        );
        assert_eq!(fill_action("app:{query}", Some("paint")), "app:paint");
        assert_eq!(
            fill_action("ticket_draft:{query}", Some("номер 4 2 1 0")),
            "ticket_draft:4210"
        );
        assert_eq!(fill_action("key:win+d", Some("x")), "key:win+d");
    }

    #[test]
    fn free_text_does_not_match() {
        let p = pack();
        assert!(p.find("какая сегодня погода в киеве").is_none());
        assert!(
            p.find("напиши письмо бухгалтеру о счетах за сентябрь")
                .is_none()
        );
        assert!(p.find("").is_none());
    }

    #[test]
    fn parses_actions_and_rejects_unknown() {
        assert_eq!(
            parse_action("key:win+d").unwrap(),
            Action::Key(vec!["win".into(), "d".into()])
        );
        assert_eq!(parse_action("volume:+10").unwrap(), Action::VolumeDelta(10));
        assert_eq!(parse_action("volume:=50").unwrap(), Action::VolumeSet(50));
        assert_eq!(
            parse_action("app:Блокнот|Notepad").unwrap(),
            Action::App(vec!["Блокнот".into(), "Notepad".into()])
        );
        assert!(parse_action("key:win+hyper").is_err());
        assert!(parse_action("teleport:home").is_err());
        assert!(is_dangerous(&Action::Shutdown));
        assert!(!is_dangerous(&Action::Lock));
    }

    #[test]
    fn similarity_and_normalize() {
        assert_eq!(normalize("Привет, Ёжик!  "), "привет ежик");
        assert!(similarity("громче", "громчи") > 0.8);
        assert!(similarity("громче", "тише") < 0.5);
    }

    #[test]
    fn user_pack_merge_replaces_by_id() {
        let mut p = pack();
        let user: QuickPack = serde_json::from_str(r#"{"version":1,"commands":[{"id":"win.desktop","name":"X","phrases":["икс"],"actions":["key:win+d"]},{"id":"my.cmd","name":"Моя","phrases":["моя команда"],"actions":["url:https://example.com"]}]}"#).unwrap();
        let n = p.commands.len();
        p.merge(user);
        assert_eq!(p.commands.len(), n + 1);
        assert_eq!(p.get("win.desktop").unwrap().name, "X");
        assert_eq!(p.find("моя команда").unwrap().id, "my.cmd");
    }

    #[tokio::test]
    async fn executor_runs_pack_actions_on_any_os() {
        let root = std::env::temp_dir().join(format!("hermes-quick-{}", std::process::id()));
        let layout = Layout::from_roots(&root, &root.join("data"));
        let ex = Executor::new(layout, "Jarvis");
        let actions: Vec<Action> = pack()
            .get("snd.mute")
            .unwrap()
            .actions
            .iter()
            .map(|a| parse_action(a).unwrap())
            .collect();
        ex.run(&actions).await.unwrap();
    }

    #[test]
    fn voice_pack_import_copies_wavs_only() {
        let root = std::env::temp_dir().join(format!("hermes-vp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src").join("Окна");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("Сворачиваю все окна.wav"), b"RIFF").unwrap();
        std::fs::write(src.join("readme.txt"), b"x").unwrap();
        let layout = Layout::from_roots(&root, &root.join("data"));
        assert_eq!(
            import_voice_pack(&layout, &root.join("src"), "Jarvis").unwrap(),
            1
        );
        assert!(clip_path(&layout, "Jarvis", "Окна/Сворачиваю все окна").is_some());
        assert!(clip_path(&layout, "Jarvis", "Окна/Нет такого").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
