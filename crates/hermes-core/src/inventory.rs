//! Host inventory (server mode, ADR 18.10.2026): which machine a Wazuh agent
//! is, how to reach it for read-only diagnostics and who owns it.
//! `<app data>/inventory.json`, edited through `PUT /control/v1/inventory`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{CoreError, Layout, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum Os {
    #[default]
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum Access {
    /// Not reachable for diagnostics (analysis from the alert only).
    #[default]
    None,
    /// OpenSSH with the service key (`hermesctl diag-key`), read-only user.
    Ssh,
    /// PowerShell remoting (Kerberos in the domain), JEA endpoint.
    Winrm,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, TS)]
#[serde(default)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Host {
    /// Wazuh `agent.name`.
    pub name: String,
    /// Other names the host appears under (FQDN, old names).
    pub aliases: Vec<String>,
    /// DNS name or IP used to connect; empty = `name`.
    pub address: String,
    pub os: Os,
    pub access: Access,
    /// SSH user (default `hermes-diag`).
    pub user: String,
    /// SSH port (default 22).
    pub port: u16,
    /// PowerShell session configuration (default `HermesDiag`, the JEA endpoint).
    pub winrm_configuration: String,
    /// Who to call: shown in the analysis.
    pub owner: String,
    /// `low` / `normal` / `high` — shown in the analysis.
    pub criticality: String,
    pub notes: String,
}

pub const DEFAULT_SSH_USER: &str = "hermes-diag";
pub const DEFAULT_JEA: &str = "HermesDiag";

impl Host {
    pub fn target(&self) -> &str {
        if self.address.trim().is_empty() {
            &self.name
        } else {
            self.address.trim()
        }
    }

    pub fn ssh_user(&self) -> &str {
        if self.user.trim().is_empty() {
            DEFAULT_SSH_USER
        } else {
            self.user.trim()
        }
    }

    pub fn ssh_port(&self) -> u16 {
        if self.port == 0 { 22 } else { self.port }
    }

    pub fn jea(&self) -> &str {
        if self.winrm_configuration.trim().is_empty() {
            DEFAULT_JEA
        } else {
            self.winrm_configuration.trim()
        }
    }

    fn matches(&self, key: &str) -> bool {
        let k = key.trim();
        !k.is_empty()
            && (self.name.eq_ignore_ascii_case(k)
                || self.address.trim().eq_ignore_ascii_case(k)
                || self.aliases.iter().any(|a| a.trim().eq_ignore_ascii_case(k))
                // web-01.deka.local matches the inventory name web-01.
                || k.split('.').next().is_some_and(|short| self.name.eq_ignore_ascii_case(short)))
    }

    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(CoreError::Other("host without a name".into()));
        }
        if !safe_target(self.target()) {
            return Err(CoreError::Other(format!(
                "{}: address `{}` must be a host name or IP",
                self.name,
                self.target()
            )));
        }
        if !safe_word(self.ssh_user()) || !safe_word(self.jea()) {
            return Err(CoreError::Other(format!(
                "{}: user / winrm_configuration may contain only letters, digits, . _ -",
                self.name
            )));
        }
        Ok(())
    }
}

/// Host name or IP: letters, digits, `.`, `-`, `:` (IPv6); never starts with `-`
/// (would be read as an ssh option).
pub fn safe_target(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

pub fn safe_word(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default, TS)]
#[serde(default)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Inventory {
    pub hosts: Vec<Host>,
}

pub fn path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("inventory.json")
}

impl Inventory {
    pub fn load(layout: &Layout) -> Inventory {
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
        for (i, h) in self.hosts.iter().enumerate() {
            h.validate()?;
            if self.hosts[..i]
                .iter()
                .any(|o| o.name.eq_ignore_ascii_case(&h.name))
            {
                return Err(CoreError::Other(format!("duplicate host `{}`", h.name)));
            }
        }
        Ok(())
    }

    /// The host for a Wazuh agent: by name, alias, address or short name;
    /// then by the agent IP.
    pub fn find(&self, agent_name: &str, agent_ip: &str) -> Option<&Host> {
        self.hosts
            .iter()
            .find(|h| h.matches(agent_name))
            .or_else(|| self.hosts.iter().find(|h| h.matches(agent_ip)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str) -> Host {
        Host {
            name: name.into(),
            access: Access::Ssh,
            ..Default::default()
        }
    }

    #[test]
    fn finds_hosts_by_name_alias_fqdn_and_ip() {
        let inv = Inventory {
            hosts: vec![
                Host {
                    aliases: vec!["www".into()],
                    address: "10.0.0.5".into(),
                    ..host("web-01")
                },
                host("db-01"),
            ],
        };
        assert_eq!(inv.find("WEB-01", "").unwrap().name, "web-01");
        assert_eq!(inv.find("www", "").unwrap().name, "web-01");
        assert_eq!(inv.find("web-01.deka.local", "").unwrap().name, "web-01");
        assert_eq!(inv.find("unknown", "10.0.0.5").unwrap().name, "web-01");
        assert!(inv.find("unknown", "10.0.0.6").is_none());
        assert!(inv.find("", "").is_none());
    }

    #[test]
    fn defaults_for_connection() {
        let h = host("web-01");
        assert_eq!(h.target(), "web-01");
        assert_eq!(h.ssh_user(), "hermes-diag");
        assert_eq!(h.ssh_port(), 22);
        assert_eq!(h.jea(), "HermesDiag");
    }

    #[test]
    fn rejects_unsafe_addresses_and_duplicates() {
        assert!(host("web-01").validate().is_ok());
        let bad = Host {
            address: "-oProxyCommand=calc".into(),
            ..host("x")
        };
        assert!(bad.validate().is_err());
        let bad = Host {
            address: "a b".into(),
            ..host("x")
        };
        assert!(bad.validate().is_err());
        let bad = Host {
            user: "root;id".into(),
            ..host("x")
        };
        assert!(bad.validate().is_err());
        let inv = Inventory {
            hosts: vec![host("a"), host("A")],
        };
        assert!(inv.validate().is_err());
        assert!(safe_target("fe80::1"));
        assert!(safe_target("db-01.deka.local"));
    }

    #[test]
    fn parses_inventory_json() {
        let inv: Inventory = serde_json::from_str(
            r#"{"hosts":[{"name":"dc-01","os":"windows","access":"winrm","owner":"IT"}]}"#,
        )
        .unwrap();
        assert_eq!(inv.hosts[0].os, Os::Windows);
        assert_eq!(inv.hosts[0].access, Access::Winrm);
    }
}
