//! Supervision of the Hermes gateway (the long-running agent process) and
//! its api_server, which our UI talks to.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::cli::run_hermes;
use crate::events::EventSender;
use crate::hermes_config::API_SERVER_PORT;
use crate::{CoreError, Layout, Result};

#[derive(Debug, Clone)]
pub struct Gateway {
    pub layout: Layout,
    client: reqwest::Client,
    api_key: Option<String>,
}

impl Gateway {
    pub fn new(layout: Layout, api_key: Option<String>) -> Gateway {
        Gateway {
            layout,
            client: reqwest::Client::new(),
            api_key,
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{API_SERVER_PORT}")
    }

    /// `GET /health` answers 200 only when the gateway and its api_server run.
    pub async fn is_healthy(&self) -> bool {
        let mut req = self
            .client
            .get(format!("{}/health", self.base_url()))
            .timeout(Duration::from_secs(3));
        if let Some(k) = &self.api_key {
            req = req.bearer_auth(k);
        }
        req.send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn start(
        &self,
        events: Option<&EventSender>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if self.is_healthy().await {
            return Ok(());
        }
        #[cfg(windows)]
        run_hermes(&self.layout, &["gateway", "start"], events, cancel).await?;
        #[cfg(not(windows))]
        {
            let _ = events;
            self.spawn_foreground()?;
        }
        self.wait_healthy(Duration::from_secs(90), cancel).await
    }

    pub async fn stop(
        &self,
        events: Option<&EventSender>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        #[cfg(windows)]
        run_hermes(&self.layout, &["gateway", "stop"], events, cancel).await?;
        #[cfg(not(windows))]
        {
            let _ = events;
            self.stop_foreground(cancel).await?;
        }
        Ok(())
    }

    /// `<app data>/gateway.pid`: the gateway we started on macOS / Linux.
    #[cfg(not(windows))]
    fn pid_file(&self) -> std::path::PathBuf {
        self.layout.app_data_dir.join("gateway.pid")
    }

    /// macOS / Linux (the server service): no launchd agent or systemd user
    /// unit of Hermes — those need a logged-in user. We run
    /// `hermes gateway run` ourselves, in its own process group so that a
    /// restart of hermesd for an update leaves it running; `watch_gateway`
    /// starts it again when it dies.
    #[cfg(not(windows))]
    fn spawn_foreground(&self) -> Result<()> {
        use std::process::Stdio;
        let logs = self.layout.hermes_home.join("logs");
        std::fs::create_dir_all(&logs)?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join("gateway-run.log"))?;
        let mut cmd = tokio::process::Command::new(self.layout.hermes_cli());
        cmd.args(["gateway", "run", "--replace", "--external-supervisor"]);
        crate::cli::hermes_env(&mut cmd, &self.layout);
        cmd.stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0);
        let child = cmd.spawn()?;
        if let Some(pid) = child.id() {
            std::fs::create_dir_all(&self.layout.app_data_dir)?;
            std::fs::write(self.pid_file(), pid.to_string())?;
            log::info!("gateway started: pid {pid}");
        }
        // Dropping the handle does not kill it; tokio reaps it when it exits.
        drop(child);
        Ok(())
    }

    #[cfg(not(windows))]
    async fn stop_foreground(&self, cancel: &CancellationToken) -> Result<()> {
        let Some(pid) = std::fs::read_to_string(self.pid_file())
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|p| *p > 1)
        else {
            return Ok(());
        };
        let kill = |sig: &str| {
            std::process::Command::new("kill")
                .args([sig, &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        if kill("-TERM") {
            for _ in 0..40 {
                if cancel.is_cancelled() || !kill("-0") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            if kill("-0") {
                log::warn!("gateway {pid} ignored SIGTERM; killing it");
                kill("-KILL");
            }
        }
        let _ = std::fs::remove_file(self.pid_file());
        Ok(())
    }

    /// Register the gateway to start at login (Scheduled Task, no admin).
    pub async fn install_autostart(
        &self,
        events: Option<&EventSender>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        run_hermes(&self.layout, &["gateway", "install"], events, cancel).await?;
        Ok(())
    }

    pub async fn wait_healthy(&self, timeout: Duration, cancel: &CancellationToken) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;
        while tokio::time::Instant::now() < deadline {
            if cancel.is_cancelled() {
                return Err(CoreError::Cancelled);
            }
            if self.is_healthy().await {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Err(CoreError::Other(format!(
            "gateway did not answer on {} within {timeout:?}",
            self.base_url()
        )))
    }
}
