//! Named access keys for the server's web panel over the network (ADR 18.10.2026):
//! each person gets their own key with a role — `read` (look only) or `admin`
//! (change settings, run fixes). Only the SHA-256 of a key is stored
//! (`<app data>/access.json`); the key itself is shown once, at creation.
//! The master `control.token` keeps working on loopback only.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::{CoreError, Layout, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../../src/types/generated/")]
pub enum Role {
    Read,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredKey {
    id: String,
    name: String,
    role: Role,
    sha256: String,
    created_unix: u64,
}

/// What the panel lists (never the key or its hash).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct AccessKeyInfo {
    pub id: String,
    pub name: String,
    pub role: Role,
    #[ts(type = "number")]
    pub created_unix: u64,
}

/// Who made a request: the key's name and role (`local` = master token).
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Caller {
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    keys: Vec<StoredKey>,
}

pub fn path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("access.json")
}

fn hash(key: &str) -> String {
    hex::encode(Sha256::digest(key.trim().as_bytes()))
}

fn load(layout: &Layout) -> Store {
    std::fs::read_to_string(path(layout))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(layout: &Layout, store: &Store) -> Result<()> {
    std::fs::create_dir_all(&layout.app_data_dir)?;
    let text = serde_json::to_string_pretty(store).map_err(|e| CoreError::Other(e.to_string()))?;
    std::fs::write(path(layout), text)?;
    Ok(())
}

pub fn list(layout: &Layout) -> Vec<AccessKeyInfo> {
    load(layout)
        .keys
        .into_iter()
        .map(|k| AccessKeyInfo {
            id: k.id,
            name: k.name,
            role: k.role,
            created_unix: k.created_unix,
        })
        .collect()
}

/// Create a key; returns its info and the key itself (shown once).
pub fn create(layout: &Layout, name: &str, role: Role) -> Result<(AccessKeyInfo, String)> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(CoreError::Other("name: 1–60 characters".into()));
    }
    let mut store = load(layout);
    if store
        .keys
        .iter()
        .any(|k| k.name.to_lowercase() == name.to_lowercase())
    {
        return Err(CoreError::Other(format!("a key named `{name}` exists")));
    }
    let key = format!("hk_{}", crate::secrets::generate_key()?);
    let id = crate::secrets::generate_key()?[..12].to_string();
    let created_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    store.keys.push(StoredKey {
        id: id.clone(),
        name: name.to_string(),
        role,
        sha256: hash(&key),
        created_unix,
    });
    save(layout, &store)?;
    Ok((
        AccessKeyInfo {
            id,
            name: name.to_string(),
            role,
            created_unix,
        },
        key,
    ))
}

pub fn remove(layout: &Layout, id: &str) -> Result<bool> {
    let mut store = load(layout);
    let before = store.keys.len();
    store.keys.retain(|k| k.id != id);
    let removed = store.keys.len() != before;
    if removed {
        save(layout, &store)?;
    }
    Ok(removed)
}

/// The caller for a presented key, if it is one of ours.
pub fn verify(layout: &Layout, key: &str) -> Option<Caller> {
    let h = hash(key);
    load(layout)
        .keys
        .into_iter()
        .find(|k| crate::control::constant_time_eq(k.sha256.as_bytes(), h.as_bytes()))
        .map(|k| Caller {
            name: k.name,
            role: k.role,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> (Layout, PathBuf) {
        let tmp = std::env::temp_dir().join(format!(
            "access-{}-{}",
            std::process::id(),
            &crate::secrets::generate_key().unwrap()[..6]
        ));
        (Layout::from_roots(&tmp, &tmp.join("data")), tmp)
    }

    #[test]
    fn keys_are_hashed_named_and_revocable() {
        let (l, tmp) = layout();
        let (info, key) = create(&l, "Иван (дежурный)", Role::Read).unwrap();
        assert!(key.starts_with("hk_") && key.len() > 60);
        let stored = std::fs::read_to_string(path(&l)).unwrap();
        assert!(!stored.contains(&key), "plaintext key must not be stored");
        assert_eq!(
            verify(&l, &key),
            Some(Caller {
                name: "Иван (дежурный)".into(),
                role: Role::Read
            })
        );
        assert!(verify(&l, "hk_wrong").is_none());
        assert!(create(&l, "иван (дежурный)", Role::Admin).is_err());
        assert!(create(&l, "  ", Role::Admin).is_err());
        assert_eq!(list(&l).len(), 1);
        assert!(remove(&l, &info.id).unwrap());
        assert!(verify(&l, &key).is_none());
        let _ = std::fs::remove_dir_all(tmp);
    }
}
