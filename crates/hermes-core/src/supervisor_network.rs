//! The HTTPS listener of the server mode on the supervisor (see `netaccess.rs`):
//! hermesd hands over the router (control API without the master token + web
//! panel) once; `network_apply` (re)starts the listener from `network.json`.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::netaccess::{self, NetworkConfig, NetworkStatus};
use crate::supervisor::Supervisor;

impl Supervisor {
    /// The app served over HTTPS (set by hermesd before the first `network_apply`).
    pub fn set_network_app(&self, app: axum::Router) {
        *self.network_app.lock().unwrap_or_else(|e| e.into_inner()) = Some(app);
    }

    pub fn network_status(&self) -> NetworkStatus {
        self.network_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set_network_status(&self, s: NetworkStatus) {
        *self.network_state.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }

    /// Stop the running listener; start a new one when enabled (server mode only).
    pub async fn network_apply(self: &Arc<Self>) {
        if let Some(old) = self
            .network_listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            old.cancel();
        }
        let cfg = NetworkConfig::load(&self.ctx.layout);
        let app = self
            .network_app
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let (true, true, Some(app)) = (self.ctx.server, cfg.enabled, app) else {
            self.set_network_status(NetworkStatus::default());
            return;
        };
        let material = match netaccess::tls_material(&self.ctx.layout, &cfg) {
            Ok(m) => m,
            Err(e) => {
                log::error!("https: {e}");
                self.set_network_status(NetworkStatus {
                    error: Some(e.to_string()),
                    ..Default::default()
                });
                return;
            }
        };
        let fingerprint = material
            .certs
            .first()
            .map(|c| netaccess::fingerprint(c.as_ref()));
        let (self_signed, names) = (material.self_signed, material.names.clone());
        let tls = match netaccess::server_config(material) {
            Ok(t) => t,
            Err(e) => {
                log::error!("https: {e}");
                self.set_network_status(NetworkStatus {
                    error: Some(e.to_string()),
                    fingerprint,
                    ..Default::default()
                });
                return;
            }
        };
        let port = cfg.listen.rsplit(':').next().unwrap_or("8443").to_string();
        let host = names.first().cloned().unwrap_or_else(|| "<server>".into());
        self.set_network_status(NetworkStatus {
            listening: true,
            url: Some(format!("https://{host}:{port}/ui/")),
            fingerprint,
            self_signed,
            names,
            error: None,
        });
        let cancel = CancellationToken::new();
        *self
            .network_listener
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
        let sup = self.clone();
        tokio::spawn(async move {
            if let Err(e) = netaccess::serve_https(app, cfg, tls, cancel).await {
                log::error!("https stopped: {e}");
                let mut st = sup.network_status();
                st.listening = false;
                st.error = Some(e.to_string());
                sup.set_network_status(st);
            }
        });
    }
}
