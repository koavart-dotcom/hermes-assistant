//! HTTPS access to the server's web panel and control API from the network
//! (server mode, ADR 18.10.2026). Off by default. When on, a TLS listener serves
//! the same panel and API as loopback, with three differences: only named access
//! keys work (no master token), the source address must be in `allowed`, and
//! every response carries HSTS. The certificate is the administrator's (PEM from
//! the domain CA) or a self-signed one generated here; its SHA-256 fingerprint is
//! shown in the panel so it can be checked before trusting it.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::extract::ConnectInfo;
use axum::http::{HeaderValue, header};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use hyper_util::service::TowerToHyperService;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::{CoreError, Layout, Result};

pub const DEFAULT_LISTEN: &str = "0.0.0.0:8443";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct NetworkConfig {
    pub enabled: bool,
    pub listen: String,
    /// Networks allowed to connect: `10.0.0.0/24`, `192.168.1.5`. Required when enabled.
    pub allowed: Vec<String>,
    /// PEM certificate chain and key (e.g. from the domain CA); empty = self-signed.
    pub cert_file: String,
    pub key_file: String,
    /// Extra names for the self-signed certificate (the computer name and its FQDN
    /// are added automatically).
    pub hostnames: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            enabled: false,
            listen: DEFAULT_LISTEN.into(),
            allowed: Vec::new(),
            cert_file: String::new(),
            key_file: String::new(),
            hostnames: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct NetworkStatus {
    pub listening: bool,
    pub url: Option<String>,
    /// SHA-256 of the certificate, `AB:CD:…`.
    pub fingerprint: Option<String>,
    pub self_signed: bool,
    /// Names the self-signed certificate is valid for.
    pub names: Vec<String>,
    pub error: Option<String>,
}

pub fn path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("network.json")
}

fn tls_dir(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("tls")
}

/// `10.0.0.0/24` or a single address.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cidr {
    net: IpAddr,
    bits: u8,
}

impl Cidr {
    pub fn parse(s: &str) -> Option<Cidr> {
        let s = s.trim();
        let (ip, bits) = match s.split_once('/') {
            Some((ip, b)) => (ip.parse::<IpAddr>().ok()?, Some(b.parse::<u8>().ok()?)),
            None => (s.parse::<IpAddr>().ok()?, None),
        };
        let max = if ip.is_ipv4() { 32 } else { 128 };
        let bits = bits.unwrap_or(max);
        (bits <= max).then_some(Cidr { net: ip, bits })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.net, ip.to_canonical()) {
            (IpAddr::V4(n), IpAddr::V4(a)) => {
                let mask = if self.bits == 0 {
                    0
                } else {
                    u32::MAX << (32 - u32::from(self.bits))
                };
                u32::from(n) & mask == u32::from(a) & mask
            }
            (IpAddr::V6(n), IpAddr::V6(a)) => {
                let mask = if self.bits == 0 {
                    0
                } else {
                    u128::MAX << (128 - u32::from(self.bits))
                };
                u128::from(n) & mask == u128::from(a) & mask
            }
            _ => false,
        }
    }
}

impl NetworkConfig {
    pub fn load(layout: &Layout) -> NetworkConfig {
        std::fs::read_to_string(path(layout))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, layout: &Layout) -> Result<()> {
        self.validate()?;
        std::fs::create_dir_all(&layout.app_data_dir)?;
        let text =
            serde_json::to_string_pretty(self).map_err(|e| CoreError::Other(e.to_string()))?;
        std::fs::write(path(layout), text)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.listen
            .parse::<SocketAddr>()
            .map_err(|_| CoreError::Other(format!("listen: `{}` is not host:port", self.listen)))?;
        if let Some(bad) = self.allowed.iter().find(|a| Cidr::parse(a).is_none()) {
            return Err(CoreError::Other(format!(
                "allowed: `{bad}` is not an address or network (10.0.0.0/24)"
            )));
        }
        if self.enabled && self.allowed.is_empty() {
            return Err(CoreError::Other(
                "allowed must list the networks of the administrators before enabling".into(),
            ));
        }
        if self.cert_file.trim().is_empty() != self.key_file.trim().is_empty() {
            return Err(CoreError::Other(
                "cert_file and key_file go together (or both empty for a self-signed one)".into(),
            ));
        }
        if let Some(bad) = self
            .hostnames
            .iter()
            .find(|h| !crate::inventory::safe_target(h))
        {
            return Err(CoreError::Other(format!(
                "hostnames: `{bad}` is not a host name"
            )));
        }
        Ok(())
    }

    pub fn allows(&self, ip: IpAddr) -> bool {
        self.allowed
            .iter()
            .filter_map(|a| Cidr::parse(a))
            .any(|c| c.contains(ip))
    }
}

/// Names for the self-signed certificate: computer name, its FQDN, the extra
/// hostnames, `localhost`.
pub fn cert_names(cfg: &NetworkConfig) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
        .to_lowercase();
    if !host.is_empty() {
        names.push(host.clone());
        if let Ok(dom) = std::env::var("USERDNSDOMAIN") {
            if !dom.is_empty() {
                names.push(format!("{host}.{}", dom.to_lowercase()));
            }
        }
    }
    names.extend(cfg.hostnames.iter().map(|h| h.trim().to_lowercase()));
    names.push("localhost".into());
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| !n.is_empty() && seen.insert(n.clone()));
    names
}

pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub struct TlsMaterial {
    pub certs: Vec<CertificateDer<'static>>,
    pub key: PrivateKeyDer<'static>,
    pub self_signed: bool,
    pub names: Vec<String>,
}

fn read_pem(
    cert: &Path,
    key: &Path,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_bytes =
        std::fs::read(cert).map_err(|e| CoreError::Other(format!("{}: {e}", cert.display())))?;
    let key_bytes =
        std::fs::read(key).map_err(|e| CoreError::Other(format!("{}: {e}", key.display())))?;
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_bytes)
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| CoreError::Other(format!("{}: {e:?}", cert.display())))?;
    if certs.is_empty() {
        return Err(CoreError::Other(format!(
            "{}: no certificate",
            cert.display()
        )));
    }
    let key = PrivateKeyDer::from_pem_slice(&key_bytes)
        .map_err(|e| CoreError::Other(format!("{}: {e:?}", key.display())))?;
    Ok((certs, key))
}

/// The administrator's certificate, or a self-signed one in `<app data>/tls/`
/// (made again when the list of names changes).
pub fn tls_material(layout: &Layout, cfg: &NetworkConfig) -> Result<TlsMaterial> {
    if !cfg.cert_file.trim().is_empty() {
        let (certs, key) = read_pem(
            Path::new(cfg.cert_file.trim()),
            Path::new(cfg.key_file.trim()),
        )?;
        return Ok(TlsMaterial {
            certs,
            key,
            self_signed: false,
            names: Vec::new(),
        });
    }
    let dir = tls_dir(layout);
    let (cert, key, names_file) = (
        dir.join("self-signed.crt"),
        dir.join("self-signed.key"),
        dir.join("self-signed.names"),
    );
    let names = cert_names(cfg);
    let wanted = names.join("\n");
    let current = std::fs::read_to_string(&names_file).unwrap_or_default();
    if !cert.is_file() || !key.is_file() || current != wanted {
        std::fs::create_dir_all(&dir)?;
        let ck = rcgen::generate_simple_self_signed(names.clone())
            .map_err(|e| CoreError::Other(format!("self-signed certificate: {e}")))?;
        std::fs::write(&cert, ck.cert.pem())?;
        std::fs::write(&key, ck.signing_key.serialize_pem())?;
        std::fs::write(&names_file, &wanted)?;
        log::info!("self-signed certificate made for {}", names.join(", "));
    }
    let (certs, key) = read_pem(&cert, &key)?;
    Ok(TlsMaterial {
        certs,
        key,
        self_signed: true,
        names,
    })
}

pub fn server_config(m: TlsMaterial) -> Result<rustls::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| CoreError::Other(format!("tls: {e}")))?
        .with_no_client_auth()
        .with_single_cert(m.certs, m.key)
        .map_err(|e| CoreError::Other(format!("tls certificate: {e}")))?;
    cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(cfg)
}

async fn hsts(mut resp: axum::response::Response) -> axum::response::Response {
    resp.headers_mut().insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000"),
    );
    resp
}

/// Serve `app` over TLS until `cancel`; connections from outside `allowed` are
/// dropped before the handshake.
pub async fn serve_https(
    app: Router,
    cfg: NetworkConfig,
    tls: rustls::ServerConfig,
    cancel: CancellationToken,
) -> Result<()> {
    let addr: SocketAddr = cfg
        .listen
        .parse()
        .map_err(|_| CoreError::Other(format!("listen `{}`", cfg.listen)))?;
    let mut tries = 0;
    let listener = loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => break l,
            Err(e) if tries < 10 => {
                tries += 1;
                log::debug!("https bind {addr}: {e}; retrying");
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
            Err(e) => return Err(e.into()),
        }
    };
    let acceptor = TlsAcceptor::from(Arc::new(tls));
    let app = app.layer(axum::middleware::map_response(hsts));
    log::info!("https panel listening on https://{addr}/ui/");
    loop {
        let (tcp, peer) = tokio::select! {
            _ = cancel.cancelled() => break,
            r = listener.accept() => match r {
                Ok(x) => x,
                Err(e) => {
                    log::warn!("https accept: {e}");
                    continue;
                }
            },
        };
        if !cfg.allows(peer.ip()) {
            log::warn!("https: refused {peer} (not in allowed)");
            continue;
        }
        let acceptor = acceptor.clone();
        let svc = app.clone().layer(axum::Extension(ConnectInfo(peer)));
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let tls = match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                acceptor.accept(tcp),
            )
            .await
            {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    log::debug!("https handshake with {peer}: {e}");
                    return;
                }
                Err(_) => return,
            };
            let conn = Builder::new(TokioExecutor::new());
            let serve = conn
                .serve_connection_with_upgrades(TokioIo::new(tls), TowerToHyperService::new(svc));
            tokio::select! {
                r = serve => if let Err(e) = r { log::debug!("https conn {peer}: {e}"); },
                _ = cancel.cancelled() => {}
            }
        });
    }
    log::info!("https panel on {addr} stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_matching() {
        let c = Cidr::parse("10.0.0.0/24").unwrap();
        assert!(c.contains("10.0.0.77".parse().unwrap()));
        assert!(!c.contains("10.0.1.1".parse().unwrap()));
        assert!(c.contains("::ffff:10.0.0.5".parse().unwrap()));
        let one = Cidr::parse("192.168.1.5").unwrap();
        assert!(one.contains("192.168.1.5".parse().unwrap()));
        assert!(!one.contains("192.168.1.6".parse().unwrap()));
        assert!(
            Cidr::parse("0.0.0.0/0")
                .unwrap()
                .contains("8.8.8.8".parse().unwrap())
        );
        assert!(
            Cidr::parse("fd00::/8")
                .unwrap()
                .contains("fd12::1".parse().unwrap())
        );
        assert!(Cidr::parse("10.0.0.0/33").is_none());
        assert!(Cidr::parse("nope").is_none());
    }

    #[test]
    fn config_validation() {
        let mut c = NetworkConfig::default();
        assert!(c.validate().is_ok());
        c.enabled = true;
        assert!(c.validate().is_err(), "enabled without allowed networks");
        c.allowed = vec!["10.0.0.0/24".into()];
        assert!(c.validate().is_ok());
        c.cert_file = "/c.pem".into();
        assert!(c.validate().is_err(), "cert without key");
        c.key_file = "/k.pem".into();
        assert!(c.validate().is_ok());
        c.hostnames = vec!["bad name".into()];
        assert!(c.validate().is_err());
    }

    #[test]
    fn self_signed_certificate_is_made_once_per_name_list() {
        let tmp = std::env::temp_dir().join(format!("netaccess-{}", std::process::id()));
        let layout = Layout::from_roots(&tmp, &tmp.join("data"));
        let cfg = NetworkConfig {
            hostnames: vec!["hermes.deka.local".into()],
            ..Default::default()
        };
        let m = tls_material(&layout, &cfg).unwrap();
        assert!(m.self_signed && m.names.contains(&"hermes.deka.local".to_string()));
        let fp1 = fingerprint(m.certs[0].as_ref());
        let fp2 = fingerprint(tls_material(&layout, &cfg).unwrap().certs[0].as_ref());
        assert_eq!(fp1, fp2, "same names → same certificate");
        assert_eq!(fp1.len(), 32 * 3 - 1);
        assert!(server_config(tls_material(&layout, &cfg).unwrap()).is_ok());
        let _ = std::fs::remove_dir_all(tmp);
    }
}
