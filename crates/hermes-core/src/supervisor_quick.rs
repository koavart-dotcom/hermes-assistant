//! Quick-command side of the [`Supervisor`]: the loaded pack (bundled +
//! user override), matching, execution with confirmation for dangerous
//! commands, voice-pack import, and a JSON-lines log.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;
use ts_rs::TS;

use crate::quick::{self, Action, Executor, QuickCommandInfo, QuickMatch, QuickPack};
use crate::supervisor::Supervisor;
use crate::{CoreError, Result};

/// Voice set used for `say:` clips (a folder under `voice-pack/`).
pub const DEFAULT_VOICE: &str = "Jarvis";

/// Result of running a quick command.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct QuickRunResult {
    pub id: String,
    pub name: String,
    pub ok: bool,
    pub error: Option<String>,
    pub stops_speech: bool,
    /// UI target requested by the command (`tickets`), handled by the shell.
    pub ui: Option<String>,
}

pub fn user_pack_path(layout: &crate::Layout) -> PathBuf {
    layout.app_data_dir.join("quick-commands.json")
}

impl Supervisor {
    /// Bundled pack merged with the user's `quick-commands.json` (if any).
    /// Re-read on every call: packs are small and edits apply instantly.
    pub fn quick_pack(&self) -> Result<QuickPack> {
        let mut pack = QuickPack::load(&self.ctx.quick_pack)?;
        let user = user_pack_path(&self.ctx.layout);
        if user.exists() {
            match QuickPack::load(&user) {
                Ok(u) => pack.merge(u),
                Err(e) => log::warn!(target: "quick", "user pack ignored: {e}"),
            }
        }
        Ok(pack)
    }

    pub fn quick_commands(&self) -> Result<Vec<QuickCommandInfo>> {
        Ok(self.quick_pack()?.infos())
    }

    pub fn quick_match(&self, text: &str) -> Result<Option<QuickMatch>> {
        Ok(self.quick_pack()?.find(text))
    }

    /// Run command `id`. Dangerous commands need `confirmed == true`.
    pub async fn quick_run(&self, id: &str, confirmed: bool) -> Result<QuickRunResult> {
        let pack = self.quick_pack()?;
        let cmd = pack
            .get(id)
            .ok_or_else(|| CoreError::Other(format!("unknown quick command `{id}`")))?
            .clone();
        let actions: Vec<Action> = cmd
            .actions
            .iter()
            .map(|a| quick::parse_action(a).map_err(CoreError::Other))
            .collect::<Result<_>>()?;
        let dangerous = cmd.confirm || actions.iter().any(quick::is_dangerous);
        if dangerous && !confirmed {
            self.quick_log(&cmd.id, "denied", "needs confirmation");
            return Err(CoreError::Other(format!(
                "`{}` требует подтверждения",
                cmd.name
            )));
        }
        let voice = self.quick_voice();
        let ex = Executor::new(self.ctx.layout.clone(), voice);
        let result = ex.run(&actions).await;
        let (ok, error) = match &result {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        self.quick_log(
            &cmd.id,
            if ok { "ok" } else { "error" },
            error.as_deref().unwrap_or(""),
        );
        Ok(QuickRunResult {
            id: cmd.id,
            name: cmd.name,
            ok,
            error,
            stops_speech: actions.iter().any(|a| matches!(a, Action::StopSpeech)),
            ui: actions.iter().find_map(|a| match a {
                Action::Ui(t) => Some(t.clone()),
                _ => None,
            }),
        })
    }

    /// Voice set: `voice-pack/<name>` — the first existing one, default Jarvis.
    pub fn quick_voice(&self) -> String {
        let dir = quick::voice_pack_dir(&self.ctx.layout);
        if dir.join(DEFAULT_VOICE).is_dir() {
            return DEFAULT_VOICE.to_string();
        }
        std::fs::read_dir(&dir)
            .ok()
            .and_then(|rd| {
                rd.flatten()
                    .find(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
            })
            .unwrap_or_else(|| DEFAULT_VOICE.to_string())
    }

    pub fn quick_import_voices(&self, src: &std::path::Path, name: &str) -> Result<usize> {
        let n = quick::import_voice_pack(&self.ctx.layout, src, name)?;
        self.quick_log(
            "voice-pack",
            "imported",
            &format!("{name}: {n} clips from {}", src.display()),
        );
        Ok(n)
    }

    fn quick_log(&self, id: &str, result: &str, detail: &str) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = json!({ "ts": ts, "id": id, "result": result, "detail": detail }).to_string();
        log::info!(target: "quick", "{line}");
        let path = quick::log_path(&self.ctx.layout);
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            use std::io::Write as _;
            let _ = writeln!(f, "{line}");
        }
    }
}

impl Supervisor {
    /// Tickets from FreeScout using the keys in Hermes' `.env`.
    pub async fn freescout_tickets(&self) -> Result<crate::freescout::TicketsSnapshot> {
        let cfg = crate::freescout::config(&self.ctx.layout)?.ok_or_else(|| {
            CoreError::Other(
                "FreeScout не настроен: задайте FREESCOUT_URL и FREESCOUT_API_KEY в «Настройки → Интеграции и ключи»".into(),
            )
        })?;
        crate::freescout::Client::new(cfg).snapshot().await
    }
}
