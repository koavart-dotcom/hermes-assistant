//! What the service does while it runs: the hermes-core supervisor in server
//! mode, the control API, the gateway watchdog and, on the very first start,
//! the Hermes Agent install.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hermes_core::bootstrap::Pin;
use hermes_core::control::{self, ControlConfig};
use hermes_core::server;
use hermes_core::setup::{SetupContext, SetupOptions};
use hermes_core::supervisor::Supervisor;
use tokio_util::sync::CancellationToken;

/// Folder with the bundled resources (`resources/hermes/pin.json` …): next to
/// the exe in Program Files, or two levels up inside the desktop install
/// (`<app>\resources\server\hermesd.exe`).
pub fn resource_root(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .take(4)
        .find(|d| d.join("resources/hermes/pin.json").is_file())
        .map(Path::to_path_buf)
}

/// `HERMES_CONTROL_PORT` or [`server::SERVER_CONTROL_PORT`].
pub fn control_port() -> u16 {
    std::env::var("HERMES_CONTROL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(server::SERVER_CONTROL_PORT)
}

pub fn build_supervisor(res: &Path, root: &Path) -> Result<Arc<Supervisor>, String> {
    let r = res.join("resources");
    let pin_path = r.join("hermes/pin.json");
    let pin: Pin = std::fs::read_to_string(&pin_path)
        .map_err(|e| format!("{}: {e}", pin_path.display()))
        .and_then(|s| serde_json::from_str(&s).map_err(|e| format!("pin.json: {e}")))?;
    Ok(Supervisor::new(Arc::new(SetupContext {
        layout: server::layout(root),
        script: r.join("hermes/install.ps1"),
        pin,
        plugins_dir: r.join("hermes-plugins"),
        quick_pack: r.join("hermes-quick-commands/default.ru.json"),
        skills_dir: r.join("hermes-skills"),
        server: true,
    })))
}

/// Run until `cancel`; then stop the gateway (a stopped service means a
/// stopped agent).
pub async fn run(sup: Arc<Supervisor>, log_dir: PathBuf, cancel: CancellationToken) {
    let layout = sup.ctx.layout.clone();
    log::info!(
        "hermesd {} starting; hermes home {}, data {}",
        env!("CARGO_PKG_VERSION"),
        layout.hermes_home.display(),
        layout.app_data_dir.display()
    );

    if layout.bootstrap_marker().exists() {
        if let Err(e) = sup.skills_ensure() {
            log::warn!("skills install: {e}");
        }
        if let Err(e) = sup.plugins_ensure().await {
            log::warn!("plugin refresh failed: {e}");
        }
    } else {
        // `hermesd install` told the administrator what is downloaded and where.
        tokio::spawn(first_setup(sup.clone(), cancel.clone()));
    }

    match control::token(&layout) {
        Ok(token) => {
            let cfg = ControlConfig {
                port: control_port(),
                token,
                app_name: server::SERVICE_DISPLAY_NAME.into(),
                app_version: env!("CARGO_PKG_VERSION").into(),
                log_dir,
            };
            let sup = sup.clone();
            tokio::spawn(async move {
                if let Err(e) = control::serve(sup, cfg).await {
                    log::error!("control api stopped: {e}");
                }
            });
        }
        Err(e) => log::error!("control api disabled, token unavailable: {e}"),
    }

    tokio::spawn(server::watch_gateway(sup.clone(), cancel.clone()));
    cancel.cancelled().await;

    log::info!("stopping: cancelling setup, stopping the gateway");
    sup.cancel_setup();
    if layout.bootstrap_marker().exists() {
        match tokio::time::timeout(Duration::from_secs(25), sup.gateway_stop()).await {
            Ok(Ok(())) => log::info!("gateway stopped"),
            Ok(Err(e)) => log::warn!("gateway stop: {e}"),
            Err(_) => log::warn!("gateway stop timed out"),
        }
    }
    log::info!("hermesd stopped");
}

/// Install Hermes Agent on the first start; retry with a growing pause when the
/// network or a download fails, until it is installed or the service stops.
async fn first_setup(sup: Arc<Supervisor>, cancel: CancellationToken) {
    let mut failures = 0u32;
    while !sup.ctx.layout.bootstrap_marker().exists() && !cancel.is_cancelled() {
        log::info!("Hermes Agent is not installed yet; running the server setup");
        let opts = SetupOptions {
            model: String::new(),
            consent: true,
            autostart: false,
        };
        match sup.start_setup(opts) {
            Ok(fut) => fut.await,
            Err(e) => log::warn!("setup did not start: {e}"),
        }
        if sup.ctx.layout.bootstrap_marker().exists() {
            break;
        }
        let wait = server::backoff(failures + 2);
        failures += 1;
        log::warn!("server setup failed ({failures} in a row); next try in {wait:?}");
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(wait) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_resources_next_to_or_above_the_exe() {
        let tmp = std::env::temp_dir().join(format!("hermesd-res-{}", std::process::id()));
        let hermes = tmp.join("resources/hermes");
        std::fs::create_dir_all(&hermes).unwrap();
        std::fs::write(hermes.join("pin.json"), "{}").unwrap();
        assert_eq!(resource_root(&tmp.join("hermesd.exe")), Some(tmp.clone()));
        assert_eq!(
            resource_root(&tmp.join("resources/server/hermesd.exe")),
            Some(tmp.clone())
        );
        assert_eq!(resource_root(Path::new("/nonexistent/x/hermesd")), None);
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
