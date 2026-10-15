//! Ollama as a sidecar: extracted from the official release zip (no installer),
//! started by us with `OLLAMA_MODELS` on the data drive, models pulled with
//! progress through its HTTP API.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use futures_util::StreamExt;
use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use crate::cli::hide_console;
use crate::events::{EventSender, SetupEvent, emit};
use crate::{CoreError, Layout, Result};

pub const OLLAMA_VERSION: &str = "v0.40.5";
/// CLI + NVIDIA libraries, no installer (Ollama docs: "Standalone CLI").
pub const OLLAMA_ZIP_BYTES: u64 = 1_471_226_485;
pub const OLLAMA_HOST: &str = "127.0.0.1:11434";

pub fn zip_url() -> String {
    format!("https://github.com/ollama/ollama/releases/download/{OLLAMA_VERSION}/ollama-windows-amd64.zip")
}

#[derive(Debug, Clone)]
pub struct Ollama {
    pub layout: Layout,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Debug, Deserialize)]
struct TagModel {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PullLine {
    #[serde(default)]
    status: String,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    completed: Option<u64>,
    #[serde(default)]
    error: Option<String>,
}

impl Ollama {
    pub fn new(layout: Layout) -> Ollama {
        Ollama { layout, client: reqwest::Client::new() }
    }

    pub fn base_url(&self) -> String {
        format!("http://{OLLAMA_HOST}")
    }

    pub fn is_installed(&self) -> bool {
        self.layout.ollama_exe().exists()
    }

    pub async fn is_running(&self) -> bool {
        self.client
            .get(format!("{}/api/version", self.base_url()))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    /// Download the release zip (if not already present) and extract it.
    pub async fn ensure_installed(&self, events: &EventSender, cancel: &CancellationToken) -> Result<()> {
        if self.is_installed() {
            emit(events, SetupEvent::Log { line: "Ollama already extracted".into() });
            return Ok(());
        }
        tokio::fs::create_dir_all(&self.layout.downloads_dir).await?;
        tokio::fs::create_dir_all(&self.layout.ollama_dir).await?;
        let archive = self.layout.downloads_dir.join(format!("ollama-windows-amd64-{OLLAMA_VERSION}.zip"));
        let complete = tokio::fs::metadata(&archive).await.map(|m| m.len() == OLLAMA_ZIP_BYTES).unwrap_or(false);
        if !complete {
            download(&self.client, &zip_url(), &archive, "ollama", events, cancel).await?;
        }
        emit(events, SetupEvent::Log { line: format!("Extracting {}", archive.display()) });
        let dest = self.layout.ollama_dir.clone();
        tokio::task::spawn_blocking(move || extract_zip(&archive, &dest))
            .await
            .map_err(|e| CoreError::Other(e.to_string()))??;
        if !self.is_installed() {
            return Err(CoreError::Other("ollama.exe not found after extraction".into()));
        }
        Ok(())
    }

    /// Start `ollama serve` unless a server already answers. The returned
    /// child must be kept alive by the caller (the supervisor).
    pub async fn start(&self) -> Result<Option<Child>> {
        if self.is_running().await {
            return Ok(None);
        }
        tokio::fs::create_dir_all(&self.layout.models_dir).await?;
        let mut cmd = Command::new(self.layout.ollama_exe());
        cmd.arg("serve")
            .env("OLLAMA_HOST", OLLAMA_HOST)
            .env("OLLAMA_MODELS", &self.layout.models_dir)
            .env("OLLAMA_KEEP_ALIVE", "24h")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false);
        hide_console(&mut cmd);
        let child = cmd.spawn()?;
        for _ in 0..40 {
            if self.is_running().await {
                return Ok(Some(child));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err(CoreError::Other("ollama serve did not become ready in 20 s".into()))
    }

    pub async fn models(&self) -> Result<Vec<String>> {
        let r: TagsResponse = self
            .client
            .get(format!("{}/api/tags", self.base_url()))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(r.models.into_iter().map(|m| m.name).collect())
    }

    pub async fn has_model(&self, tag: &str) -> bool {
        let want = normalize_tag(tag);
        self.models().await.map(|m| m.iter().any(|n| normalize_tag(n) == want)).unwrap_or(false)
    }

    /// `POST /api/pull` with streaming NDJSON progress.
    pub async fn pull(&self, tag: &str, events: &EventSender, cancel: &CancellationToken) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/api/pull", self.base_url()))
            .json(&serde_json::json!({ "model": tag, "stream": true }))
            .send()
            .await?
            .error_for_status()?;
        let mut stream = resp.bytes_stream();
        let mut buf = Vec::new();
        let mut last_status = String::new();
        loop {
            let chunk = tokio::select! {
                _ = cancel.cancelled() => return Err(CoreError::Cancelled),
                c = stream.next() => match c { Some(c) => c?, None => break },
            };
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = buf.drain(..=pos).collect();
                let Ok(text) = std::str::from_utf8(&line) else { continue };
                let Ok(p) = serde_json::from_str::<PullLine>(text.trim()) else { continue };
                if let Some(err) = p.error {
                    return Err(CoreError::Other(format!("ollama pull: {err}")));
                }
                if p.status != last_status {
                    last_status = p.status.clone();
                    emit(events, SetupEvent::Log { line: format!("ollama: {}", p.status) });
                }
                if let (Some(total), Some(done)) = (p.total, p.completed) {
                    emit(events, SetupEvent::Progress { label: format!("model {tag}"), received: done, total: Some(total) });
                }
            }
        }
        Ok(())
    }
}

/// `qwen3.5` and `qwen3.5:latest` are the same model.
pub fn normalize_tag(tag: &str) -> String {
    match tag.split_once(':') {
        Some((name, "latest")) => name.to_string(),
        _ => tag.to_string(),
    }
}

/// Stream a URL to disk with progress events roughly every megabyte.
pub async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    label: &str,
    events: &EventSender,
    cancel: &CancellationToken,
) -> Result<()> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let total = resp.content_length();
    let tmp = dest.with_extension("part");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    let mut received: u64 = 0;
    let mut last_report: u64 = 0;
    loop {
        let chunk = tokio::select! {
            _ = cancel.cancelled() => { drop(file); let _ = tokio::fs::remove_file(&tmp).await; return Err(CoreError::Cancelled); }
            c = stream.next() => match c { Some(c) => c?, None => break },
        };
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        if received - last_report >= 1_000_000 {
            last_report = received;
            emit(events, SetupEvent::Progress { label: label.to_string(), received, total });
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, dest).await?;
    emit(events, SetupEvent::Progress { label: label.to_string(), received, total: Some(total.unwrap_or(received)) });
    Ok(())
}

/// Extract a zip archive into `dest`, refusing entries that escape it.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let out: PathBuf = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut f)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_latest_tag() {
        assert_eq!(normalize_tag("qwen3.5:latest"), "qwen3.5");
        assert_eq!(normalize_tag("qwen3.5"), "qwen3.5");
        assert_eq!(normalize_tag("gemma4:12b-it-qat"), "gemma4:12b-it-qat");
    }

    #[test]
    fn zip_url_points_to_pinned_release() {
        assert!(zip_url().contains(OLLAMA_VERSION));
        assert!(zip_url().ends_with("ollama-windows-amd64.zip"));
    }

    #[test]
    fn extracts_zip_round_trip() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("hermes-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("a.zip");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("bin/ollama.exe", opts).unwrap();
            w.write_all(b"MZ").unwrap();
            w.finish().unwrap();
        }
        let out = dir.join("out");
        extract_zip(&archive, &out).unwrap();
        assert_eq!(std::fs::read(out.join("bin/ollama.exe")).unwrap(), b"MZ");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
