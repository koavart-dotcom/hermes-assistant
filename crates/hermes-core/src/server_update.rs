//! Self-update of the server service (hermesd). Each release publishes
//! `hermesd-windows-x64.zip` (hermesd.exe + resources) and
//! `hermesd-macos-arm64.zip` (hermesd + resources), signed with the same
//! minisign key as the desktop updater, and `hermesd.json` describing it. The
//! service checks the manifest every few hours; when a newer version is out (and
//! auto-update is on, or a person pressed «Обновить» in the panel) it downloads
//! the package, verifies the signature, swaps the files in its Program Files
//! folder and asks to be restarted (the SCM restarts it with the new exe).
//!
//! A running exe can be renamed (Windows and Unix alike), so the swap is: hermesd.exe →
//! hermesd.old.exe, new hermesd.exe in place, resource folders replaced; the old
//! exe is deleted on the next start. The service account has Modify on its
//! program folder for this (granted by `hermesd install`; on macOS / Linux the
//! service user owns it). On macOS / Linux the names are `hermesd`, `hermesd.old`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::supervisor::Supervisor;
use crate::{CoreError, Layout, Result};

const RELEASES: &str =
    "https://github.com/koavart-dotcom/hermes-assistant/releases/latest/download";
/// Manifest of this platform's package: `hermesd.json` (Windows, the first
/// one), `hermesd-macos-arm64.json`, `hermesd-linux-x64.json`.
pub const MANIFEST_FILE: &str = if cfg!(windows) {
    "hermesd.json"
} else if cfg!(target_os = "macos") {
    "hermesd-macos-arm64.json"
} else {
    "hermesd-linux-x64.json"
};
/// `plugins.updater.pubkey` of tauri.conf.json (checked by a test).
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEJDNjcyRDRFQUJCNjkyM0IKUldRN2tyYXJUaTFudkUyV09CbzdTQ09zZWczMVBJNlpxYXNZSWkzZTJ6WmpQOHg5aDZSNXlVYisK";
pub const PACKAGE_EXE: &str = if cfg!(windows) {
    "hermesd.exe"
} else {
    "hermesd"
};
const MAX_PACKAGE: u64 = 300 * 1024 * 1024;

/// `<app data>/update.json`: settings and the version that ran last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct UpdateConfig {
    pub auto: bool,
    pub check_hours: u32,
    /// Version of the previous start (to report «updated from → to»).
    #[ts(skip)]
    pub last_version: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        UpdateConfig {
            auto: true,
            check_hours: 6,
            last_version: String::new(),
        }
    }
}

pub fn config_path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("update.json")
}

impl UpdateConfig {
    pub fn load(layout: &Layout) -> UpdateConfig {
        std::fs::read_to_string(config_path(layout))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, layout: &Layout) -> Result<()> {
        if !(1..=168).contains(&self.check_hours) {
            return Err(CoreError::Other("check_hours: 1…168".into()));
        }
        std::fs::create_dir_all(&layout.app_data_dir)?;
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Other(e.to_string()))?;
        std::fs::write(config_path(layout), text)?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    /// URL of hermesd-windows-x64.zip.
    pub zip: String,
    /// Contents of the `.sig` file made by `tauri signer sign` (base64).
    pub signature: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct UpdateStatus {
    pub current: String,
    pub latest: Option<String>,
    pub available: bool,
    #[ts(type = "number | null")]
    pub checked_unix: Option<u64>,
    /// `idle` / `checking` / `downloading` / `installing` / `restarting`.
    pub state: String,
    pub error: Option<String>,
    pub auto: bool,
    pub check_hours: u32,
    /// Service mode with a writable program folder (updates possible).
    pub supported: bool,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.trim().trim_start_matches('v').split(['.', '-', '+']);
    Some((
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    ))
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// Verify a `tauri signer sign` signature with [`PUBKEY`].
pub fn verify(data: &[u8], signature_b64: &str, pubkey_b64: &str) -> Result<()> {
    let b64 = |s: &str| -> Result<String> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(s.trim())
            .map_err(|e| CoreError::Other(format!("signature base64: {e}")))?;
        String::from_utf8(bytes).map_err(|e| CoreError::Other(format!("signature text: {e}")))
    };
    let pk = minisign_verify::PublicKey::decode(&b64(pubkey_b64)?)
        .map_err(|e| CoreError::Other(format!("public key: {e}")))?;
    let sig = minisign_verify::Signature::decode(&b64(signature_b64)?)
        .map_err(|e| CoreError::Other(format!("signature: {e}")))?;
    pk.verify(data, &sig, false)
        .map_err(|e| CoreError::Other(format!("signature does not match: {e}")))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// The old exe left by the previous update (deleted at start).
pub fn old_exe(exe: &Path) -> PathBuf {
    exe.with_file_name(if cfg!(windows) {
        "hermesd.old.exe"
    } else {
        "hermesd.old"
    })
}

/// Swap the running installation for the unpacked package in `staging`.
pub fn install_package(staging: &Path, exe: &Path) -> Result<()> {
    let new_exe = staging.join(PACKAGE_EXE);
    if !new_exe.is_file() || !staging.join("resources/hermes/pin.json").is_file() {
        return Err(CoreError::Other(format!(
            "package is incomplete ({PACKAGE_EXE} or resources/hermes missing)"
        )));
    }
    let dir = exe
        .parent()
        .ok_or_else(|| CoreError::Other("exe has no folder".into()))?;
    let old = old_exe(exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old)
        .map_err(|e| CoreError::Other(format!("rename {}: {e}", exe.display())))?;
    if let Err(e) = std::fs::copy(&new_exe, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(CoreError::Other(format!("copy new {PACKAGE_EXE}: {e}")));
    }
    // Zip entries carry no Unix mode: make the new binary executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| CoreError::Other(format!("chmod {}: {e}", exe.display())))?;
    }
    for d in crate::server::RESOURCE_DIRS {
        let from = staging.join("resources").join(d);
        if !from.is_dir() {
            continue;
        }
        let to = dir.join("resources").join(d);
        if to.exists() {
            std::fs::remove_dir_all(&to)
                .map_err(|e| CoreError::Other(format!("remove {}: {e}", to.display())))?;
        }
        copy_dir(&from, &to).map_err(|e| CoreError::Other(format!("copy resources/{d}: {e}")))?;
    }
    Ok(())
}

async fn fetch_manifest(url: &str) -> Result<Manifest> {
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .get(url)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(CoreError::Other(format!(
            "manifest: HTTP {}",
            resp.status()
        )));
    }
    resp.json::<Manifest>()
        .await
        .map_err(|e| CoreError::Other(format!("manifest: {e}")))
}

async fn download(url: &str) -> Result<Vec<u8>> {
    let resp = reqwest::Client::builder()
        .timeout(Duration::from_secs(900))
        .build()?
        .get(url)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(CoreError::Other(format!("package: HTTP {}", resp.status())));
    }
    if resp.content_length().is_some_and(|n| n > MAX_PACKAGE) {
        return Err(CoreError::Other("package is too large".into()));
    }
    let bytes = resp.bytes().await?;
    if bytes.len() as u64 > MAX_PACKAGE {
        return Err(CoreError::Other("package is too large".into()));
    }
    Ok(bytes.to_vec())
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn manifest_url() -> String {
    std::env::var("HERMES_UPDATE_URL").unwrap_or_else(|_| format!("{RELEASES}/{MANIFEST_FILE}"))
}

impl Supervisor {
    fn update_set(&self, f: impl FnOnce(&mut UpdateStatus)) {
        let mut s = self.update_state.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut s);
    }

    /// The running exe when it can update itself (service mode, `hermesd.exe`).
    fn updatable_exe(&self) -> Option<PathBuf> {
        if !self.ctx.server {
            return None;
        }
        std::env::current_exe().ok().filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(PACKAGE_EXE))
        })
    }

    pub fn update_status(&self) -> UpdateStatus {
        let cfg = UpdateConfig::load(&self.ctx.layout);
        let mut s = self
            .update_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        s.current = current_version().into();
        if s.state.is_empty() {
            s.state = "idle".into();
        }
        s.auto = cfg.auto;
        s.check_hours = cfg.check_hours;
        s.supported = self.updatable_exe().is_some();
        s
    }

    pub fn update_config_set(&self, auto: bool, check_hours: u32) -> Result<UpdateStatus> {
        let mut cfg = UpdateConfig::load(&self.ctx.layout);
        cfg.auto = auto;
        cfg.check_hours = check_hours;
        cfg.save(&self.ctx.layout)?;
        Ok(self.update_status())
    }

    /// Look at the published manifest.
    pub async fn update_check(&self) -> Result<Option<Manifest>> {
        self.update_set(|s| {
            s.state = "checking".into();
            s.error = None;
        });
        let r = fetch_manifest(&manifest_url()).await;
        let checked = now_unix();
        match r {
            Ok(m) => {
                let newer = is_newer(&m.version, current_version());
                self.update_set(|s| {
                    s.state = "idle".into();
                    s.latest = Some(m.version.clone());
                    s.available = newer;
                    s.checked_unix = Some(checked);
                });
                Ok(newer.then_some(m))
            }
            Err(e) => {
                self.update_set(|s| {
                    s.state = "idle".into();
                    s.error = Some(e.to_string());
                    s.checked_unix = Some(checked);
                });
                Err(e)
            }
        }
    }

    /// Download, verify and install `m`; on success the service must restart.
    pub async fn update_apply(&self, m: &Manifest) -> Result<()> {
        let exe = self.updatable_exe().ok_or_else(|| {
            CoreError::Other("self-update works only for the hermesd service".into())
        })?;
        if self.setup_running() {
            return Err(CoreError::Other("Hermes setup is running".into()));
        }
        if !self
            .fix_busy
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
        {
            return Err(CoreError::Other("a fix is running".into()));
        }
        let fail = |e: CoreError| {
            self.update_set(|s| {
                s.state = "idle".into();
                s.error = Some(e.to_string());
            });
            e
        };
        self.update_set(|s| {
            s.state = "downloading".into();
            s.error = None;
        });
        let bytes = download(&m.zip).await.map_err(fail)?;
        verify(&bytes, &m.signature, PUBKEY).map_err(fail)?;
        self.update_set(|s| s.state = "installing".into());
        let dir = self.ctx.layout.downloads_dir.join("hermesd-update");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| fail(e.into()))?;
        let zip = dir.join("package.zip");
        std::fs::write(&zip, &bytes).map_err(|e| fail(e.into()))?;
        let staging = dir.join("staging");
        crate::vosk::unzip(&zip, &staging, false).map_err(fail)?;
        install_package(&staging, &exe).map_err(fail)?;
        let _ = std::fs::remove_dir_all(&dir);
        log::info!(
            "service updated on disk: {} → {}; restarting",
            current_version(),
            m.version
        );
        self.update_set(|s| s.state = "restarting".into());
        Ok(())
    }

    /// Ask hermesd to exit so the SCM starts the new version.
    pub fn request_restart(&self) {
        self.restart_requested.cancel();
    }

    /// At start: drop the old exe, report «updated from → to» once.
    pub fn update_on_start(&self) -> Option<String> {
        if let Some(exe) = self.updatable_exe() {
            let _ = std::fs::remove_file(old_exe(&exe));
        }
        let layout = &self.ctx.layout;
        let mut cfg = UpdateConfig::load(layout);
        let prev = std::mem::replace(&mut cfg.last_version, current_version().into());
        if prev != current_version() {
            let _ = cfg.save(layout);
        }
        (!prev.is_empty() && prev != current_version())
            .then(|| format!("Hermes Server обновлён: {prev} → {}", current_version()))
    }
}

/// Every `check_hours`: check, and install when auto-update is on.
pub async fn auto_update_loop(sup: Arc<Supervisor>, cancel: CancellationToken) {
    let mut wait = Duration::from_secs(120);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(wait) => {}
        }
        let cfg = UpdateConfig::load(&sup.ctx.layout);
        wait = Duration::from_secs(u64::from(cfg.check_hours.max(1)) * 3600);
        if sup.updatable_exe().is_none() {
            continue;
        }
        match sup.update_check().await {
            Ok(Some(m)) if cfg.auto => match sup.update_apply(&m).await {
                Ok(()) => {
                    sup.request_restart();
                    return;
                }
                Err(e) => log::warn!("auto-update to {} failed: {e}", m.version),
            },
            Ok(Some(m)) => log::info!("update {} available (auto-update is off)", m.version),
            Ok(None) => {}
            Err(e) => log::warn!("update check: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(is_newer("0.33.0", "0.32.0"));
        assert!(is_newer("v1.0.0", "0.99.9"));
        assert!(!is_newer("0.32.0", "0.32.0"));
        assert!(!is_newer("0.31.5", "0.32.0"));
        assert!(!is_newer("garbage", "0.32.0"));
    }

    #[test]
    fn pubkey_matches_tauri_conf() {
        let conf = include_str!("../../../src-tauri/tauri.conf.json");
        assert!(conf.contains(&format!("\"pubkey\": \"{PUBKEY}\"")));
    }

    #[test]
    fn rejects_bad_signatures() {
        assert!(verify(b"data", "bm90IGEgc2ln", PUBKEY).is_err());
        assert!(verify(b"data", "!!!", PUBKEY).is_err());
    }

    /// `HERMES_SIGNED_FILE=<file>` with `<file>.sig` from `tauri signer sign`.
    #[test]
    #[ignore]
    fn verifies_a_real_signature() {
        let f = std::env::var("HERMES_SIGNED_FILE").expect("HERMES_SIGNED_FILE");
        let data = std::fs::read(&f).unwrap();
        let sig = std::fs::read_to_string(format!("{f}.sig")).unwrap();
        verify(&data, &sig, PUBKEY).unwrap();
        let mut tampered = data.clone();
        tampered.push(0);
        assert!(verify(&tampered, &sig, PUBKEY).is_err());
    }

    #[test]
    fn swaps_exe_and_resources() {
        let tmp = std::env::temp_dir().join(format!("hermesd-upd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let prog = tmp.join("prog");
        let stage = tmp.join("stage");
        for (root, tag) in [(&prog, "old"), (&stage, "new")] {
            std::fs::create_dir_all(root.join("resources/hermes")).unwrap();
            std::fs::create_dir_all(root.join("resources/server-ui")).unwrap();
            std::fs::write(root.join(PACKAGE_EXE), tag).unwrap();
            std::fs::write(root.join("resources/hermes/pin.json"), tag).unwrap();
        }
        std::fs::write(prog.join("resources/server-ui/stale.js"), "x").unwrap();
        let exe = prog.join(PACKAGE_EXE);
        install_package(&stage, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old_exe(&exe)).unwrap(), "old");
        assert_eq!(
            std::fs::read_to_string(prog.join("resources/hermes/pin.json")).unwrap(),
            "new"
        );
        assert!(!prog.join("resources/server-ui/stale.js").exists());
        // An incomplete package changes nothing.
        let empty = tmp.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(install_package(&empty, &exe).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
