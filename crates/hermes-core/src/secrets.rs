//! Secrets: the API key our shell uses to talk to the Hermes api_server.
//! Stored in the OS credential store (Windows Credential Manager via
//! `keyring`), with a file fallback when the store is unavailable.

use std::path::Path;

use crate::{CoreError, Layout, Result};

const SERVICE: &str = "Hermes";
const API_KEY_USER: &str = "api_server_key";

pub fn generate_key() -> Result<String> {
    let mut buf = [0u8; 32];
    getrandom::fill(&mut buf).map_err(|e| CoreError::Other(format!("getrandom: {e}")))?;
    Ok(hex::encode(buf))
}

fn fallback_path(layout: &Layout) -> std::path::PathBuf {
    layout.app_data_dir.join("secrets").join("api_server_key")
}

fn read_fallback(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn write_fallback(path: &Path, key: &str) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, key)?;
    Ok(())
}

/// Return the api_server key, creating and storing it on first use.
pub fn api_server_key(layout: &Layout) -> Result<String> {
    let fallback = fallback_path(layout);
    match keyring::Entry::new(SERVICE, API_KEY_USER) {
        Ok(entry) => match entry.get_password() {
            Ok(k) if !k.is_empty() => Ok(k),
            Ok(_) | Err(keyring::Error::NoEntry) => {
                let key = read_fallback(&fallback)
                    .map(Ok)
                    .unwrap_or_else(generate_key)?;
                if entry.set_password(&key).is_err() {
                    write_fallback(&fallback, &key)?;
                }
                Ok(key)
            }
            Err(e) => {
                tracing::warn!("keyring unavailable ({e}); using file fallback");
                file_only(&fallback)
            }
        },
        Err(e) => {
            tracing::warn!("keyring entry error ({e}); using file fallback");
            file_only(&fallback)
        }
    }
}

fn file_only(path: &Path) -> Result<String> {
    if let Some(k) = read_fallback(path) {
        return Ok(k);
    }
    let key = generate_key()?;
    write_fallback(path, &key)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_is_64_hex_chars() {
        let k = generate_key().unwrap();
        assert_eq!(k.len(), 64);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(k, generate_key().unwrap());
    }

    #[test]
    fn file_fallback_round_trips() {
        let dir = std::env::temp_dir().join(format!("hermes-secret-{}", std::process::id()));
        let p = dir.join("secrets").join("api_server_key");
        let a = file_only(&p).unwrap();
        let b = file_only(&p).unwrap();
        assert_eq!(a, b);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
