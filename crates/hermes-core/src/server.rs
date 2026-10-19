//! Server mode (`hermesd`: a Windows service on Windows Server, a launchd
//! daemon on macOS, a systemd unit on Linux): where its data
//! lives and the loop that keeps the Hermes gateway up without a logged-in user.
//!
//! The desktop app keeps everything in the user's `%LOCALAPPDATA%`; the service
//! uses one machine-wide root, `%ProgramData%\Hermes Assistant Server`
//! (`HERMES_SERVER_ROOT` overrides it), so an administrator finds the control
//! token and the logs in a known place whatever account the service runs as.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::Layout;
use crate::supervisor::Supervisor;

/// Name of the Windows service and of the folders under Program Files / ProgramData.
pub const SERVICE_NAME: &str = "HermesAssistant";
pub const SERVICE_DISPLAY_NAME: &str = "Hermes Assistant Server";
/// Control API port of the service: 8643 stays with the desktop app, so the
/// two can run on one machine (the app as the administrator's console).
pub const SERVER_CONTROL_PORT: u16 = 8645;
/// Resource folders the service needs (copied by `hermesd install`, replaced by the
/// self-update; the Vosk DLLs and the desktop app are not part of it).
pub const RESOURCE_DIRS: &[&str] = &[
    "hermes",
    "hermes-plugins",
    "hermes-skills",
    "hermes-persona",
    "hermes-quick-commands",
    "server-ui",
];

/// How often the gateway health is checked.
pub const WATCH_INTERVAL: Duration = Duration::from_secs(60);

/// launchd label / systemd unit name of the service on macOS and Linux.
pub const UNIX_SERVICE_ID: &str = "ua.koavart.hermesd";

/// `HERMES_SERVER_ROOT`, or the platform default: `%ProgramData%\Hermes
/// Assistant Server` on Windows, `/usr/local/var/hermes-server` on macOS,
/// `/var/lib/hermes-server` on Linux (no spaces: Hermes' shell installer and
/// uv run there).
pub fn server_root() -> PathBuf {
    if let Some(p) = std::env::var_os("HERMES_SERVER_ROOT") {
        return PathBuf::from(p);
    }
    if cfg!(windows) {
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        program_data.join(SERVICE_DISPLAY_NAME)
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/usr/local/var/hermes-server")
    } else {
        PathBuf::from("/var/lib/hermes-server")
    }
}

/// Where `hermesd install` puts the program on macOS / Linux.
pub fn unix_program_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/usr/local/hermes-server")
    } else {
        PathBuf::from("/opt/hermes-server")
    }
}

/// Layout of the service: Hermes home, our data and downloads all under `root`.
pub fn layout(root: &Path) -> Layout {
    Layout::from_roots(root, &root.join("data"))
}

/// Delay before the next restart attempt after `failures` failed ones:
/// 1, 2, 4 … minutes, at most 30.
pub fn backoff(failures: u32) -> Duration {
    let minutes = 1u64 << failures.min(5);
    Duration::from_secs(60 * minutes.min(30))
}

/// Keep the gateway running until `cancel`: check health every
/// [`WATCH_INTERVAL`], restart it when it is down. Before the first setup
/// (Hermes not installed) nothing is started.
pub async fn watch_gateway(sup: Arc<Supervisor>, cancel: CancellationToken) {
    let mut failures = 0u32;
    let mut wait = Duration::from_secs(5);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(wait) => {}
        }
        wait = WATCH_INTERVAL;
        if !sup.ctx.layout.bootstrap_marker().exists() || sup.setup_running() {
            continue;
        }
        if sup.gateway_healthy().await {
            failures = 0;
            continue;
        }
        log::warn!("gateway is down; starting it");
        match sup.gateway_start().await {
            Ok(()) => {
                log::info!("gateway started");
                failures = 0;
            }
            Err(e) => {
                wait = backoff(failures);
                failures += 1;
                log::error!(
                    "gateway start failed ({failures} in a row), next try in {wait:?}: {e}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_layout_keeps_everything_under_the_root() {
        let l = layout(Path::new("/pd/Hermes Assistant Server"));
        assert_eq!(
            l.hermes_home,
            Path::new("/pd/Hermes Assistant Server/hermes")
        );
        assert_eq!(
            l.app_data_dir,
            Path::new("/pd/Hermes Assistant Server/ua.koavart.hermes")
        );
        assert_eq!(
            l.downloads_dir,
            Path::new("/pd/Hermes Assistant Server/data/downloads")
        );
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff(0), Duration::from_secs(60));
        assert_eq!(backoff(1), Duration::from_secs(120));
        assert_eq!(backoff(3), Duration::from_secs(480));
        assert_eq!(backoff(10), Duration::from_secs(1800));
    }
}
