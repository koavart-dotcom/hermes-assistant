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
        run_hermes(&self.layout, &["gateway", "start"], events, cancel).await?;
        self.wait_healthy(Duration::from_secs(90), cancel).await
    }

    pub async fn stop(
        &self,
        events: Option<&EventSender>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        run_hermes(&self.layout, &["gateway", "stop"], events, cancel).await?;
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
