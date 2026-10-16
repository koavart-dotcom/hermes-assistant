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
/// Ollama's conventional port. Other software may squat on it (on the main
/// PC VS Code listens on 11434 and never answers), so the port actually used
/// is chosen by [`Ollama::ensure_port`] and remembered in `ollama-port`.
pub const DEFAULT_PORT: u16 = 11434;
/// How many ports above [`DEFAULT_PORT`] to try.
pub const PORT_TRIES: u16 = 10;
/// `OLLAMA_CONTEXT_LENGTH` for the sidecar (tokens). See `start`.
pub const OLLAMA_CONTEXT_LENGTH: &str = "65536";

pub fn zip_url() -> String {
    format!(
        "https://github.com/ollama/ollama/releases/download/{OLLAMA_VERSION}/ollama-windows-amd64.zip"
    )
}

#[derive(Debug, Clone)]
pub struct Ollama {
    pub layout: Layout,
    client: reqwest::Client,
    /// `127.0.0.1:<port>` — the port chosen by `ensure_port` (or the default).
    host: String,
}

#[derive(Debug, Deserialize)]
struct VersionResponse {
    #[serde(default)]
    version: String,
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

/// `<app_data_dir>/ollama-port` — the port chosen by `ensure_port`.
pub fn port_file(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("ollama-port")
}

#[cfg(test)]
mod port_tests {
    use super::*;

    #[tokio::test]
    async fn skips_a_port_that_accepts_connections() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let taken = listener.local_addr().unwrap().port();
        let free = first_free_port(taken, 5).await.unwrap();
        assert_ne!(free, taken);
        assert!(free > taken && free < taken + 5);
    }

    #[tokio::test]
    async fn new_reads_remembered_port() {
        let root = std::env::temp_dir().join(format!("hermes-port-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        assert_eq!(Ollama::new(layout.clone()).host(), "127.0.0.1:11434");
        std::fs::create_dir_all(&layout.app_data_dir).unwrap();
        std::fs::write(port_file(&layout), "11436\n").unwrap();
        let o = Ollama::new(layout);
        assert_eq!(o.host(), "127.0.0.1:11436");
        assert_eq!(o.base_url(), "http://127.0.0.1:11436");
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// First port in `start..start+tries` with nothing listening on loopback.
pub async fn first_free_port(start: u16, tries: u16) -> Option<u16> {
    for port in start..start.saturating_add(tries) {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let connect = tokio::net::TcpStream::connect(addr);
        match tokio::time::timeout(std::time::Duration::from_millis(700), connect).await {
            Ok(Ok(_)) => continue,                    // something accepted: taken
            Ok(Err(_)) | Err(_) => return Some(port), // refused or silent: free
        }
    }
    None
}

impl Ollama {
    pub fn new(layout: Layout) -> Ollama {
        let port = std::fs::read_to_string(port_file(&layout))
            .ok()
            .and_then(|s| s.trim().parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        Ollama {
            layout,
            client: reqwest::Client::new(),
            host: format!("127.0.0.1:{port}"),
        }
    }

    /// `127.0.0.1:<port>`.
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.host)
    }

    pub fn is_installed(&self) -> bool {
        self.layout.ollama_exe().exists()
    }

    /// True only when a real Ollama answers `/api/version` on our port
    /// (a foreign listener that accepts and never replies does not count).
    pub async fn is_running(&self) -> bool {
        let Ok(resp) = self
            .client
            .get(format!("{}/api/version", self.base_url()))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        else {
            return false;
        };
        resp.status().is_success()
            && resp
                .json::<VersionResponse>()
                .await
                .map(|v| !v.version.is_empty())
                .unwrap_or(false)
    }

    /// Make sure our port is usable: keep it when our Ollama already answers,
    /// otherwise pick the first free port from the default upwards and
    /// remember it for every later start and for the Hermes config.
    pub async fn ensure_port(&mut self, events: &EventSender) -> Result<()> {
        if self.is_running().await {
            return Ok(());
        }
        let port = first_free_port(DEFAULT_PORT, PORT_TRIES)
            .await
            .ok_or_else(|| {
                CoreError::Other(format!(
                    "no free port for Ollama in {DEFAULT_PORT}..{}",
                    DEFAULT_PORT + PORT_TRIES
                ))
            })?;
        if port != DEFAULT_PORT {
            emit(
                events,
                SetupEvent::Log {
                    line: format!(
                        "port {DEFAULT_PORT} is taken by another program (not Ollama); using {port}"
                    ),
                },
            );
        }
        self.host = format!("127.0.0.1:{port}");
        let file = port_file(&self.layout);
        if let Some(p) = file.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        tokio::fs::write(&file, port.to_string()).await?;
        Ok(())
    }

    /// Download the release zip (if not already present) and extract it.
    pub async fn ensure_installed(
        &self,
        events: &EventSender,
        cancel: &CancellationToken,
    ) -> Result<()> {
        if self.is_installed() {
            emit(
                events,
                SetupEvent::Log {
                    line: "Ollama already extracted".into(),
                },
            );
            return Ok(());
        }
        tokio::fs::create_dir_all(&self.layout.downloads_dir).await?;
        tokio::fs::create_dir_all(&self.layout.ollama_dir).await?;
        let archive = self
            .layout
            .downloads_dir
            .join(format!("ollama-windows-amd64-{OLLAMA_VERSION}.zip"));
        let complete = tokio::fs::metadata(&archive)
            .await
            .map(|m| m.len() == OLLAMA_ZIP_BYTES)
            .unwrap_or(false);
        if !complete {
            download(&self.client, &zip_url(), &archive, "ollama", events, cancel).await?;
        }
        emit(
            events,
            SetupEvent::Log {
                line: format!("Extracting {}", archive.display()),
            },
        );
        let dest = self.layout.ollama_dir.clone();
        tokio::task::spawn_blocking(move || extract_zip(&archive, &dest))
            .await
            .map_err(|e| CoreError::Other(e.to_string()))??;
        if !self.is_installed() {
            return Err(CoreError::Other(
                "ollama.exe not found after extraction".into(),
            ));
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
        // `ollama serve` logs to stderr; keep it in a file so a failed start
        // can be diagnosed (the first run on the PC died silently).
        let log_path = self.log_file();
        if let Some(p) = log_path.parent() {
            tokio::fs::create_dir_all(p).await?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let log_err = log.try_clone()?;
        let mut cmd = Command::new(self.layout.ollama_exe());
        cmd.arg("serve")
            .env("OLLAMA_HOST", &self.host)
            .env("OLLAMA_MODELS", &self.layout.models_dir)
            .env("OLLAMA_KEEP_ALIVE", "24h")
            // Hermes sends ~14k tokens of system prompt + tool schemas before
            // the conversation; Ollama's default 4096 context silently cut the
            // tools off ("truncating input prompt"), so the model only chatted.
            // Hermes documents 64k as the minimum for agentic work; qwen3.5 is
            // hybrid-attention and fits 64k in 7.3 GB VRAM on the 3080 Ti.
            .env("OLLAMA_CONTEXT_LENGTH", OLLAMA_CONTEXT_LENGTH)
            .env("OLLAMA_FLASH_ATTENTION", "1")
            .current_dir(&self.layout.ollama_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .kill_on_drop(false);
        hide_console(&mut cmd);
        let mut child = cmd.spawn()?;
        // First start on Windows can take a while (Defender scans ~1 GB of
        // CUDA libraries); poll for up to 90 s but fail fast if it exits.
        for _ in 0..180 {
            if let Some(status) = child.try_wait()? {
                return Err(CoreError::Other(format!(
                    "ollama serve exited with {status}; last log lines: {}",
                    self.log_tail(8)
                )));
            }
            if self.is_running().await {
                return Ok(Some(child));
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        Err(CoreError::Other(format!(
            "ollama serve did not become ready in 90 s; last log lines: {}",
            self.log_tail(8)
        )))
    }

    /// Stop `ollama serve` processes started from our `ollama_dir` (Windows
    /// only), so a fresh `start` picks up changed environment such as the
    /// context length. Foreign Ollama installs are left alone. Waits until
    /// our port stops answering.
    pub async fn stop_owned(&self) -> Result<()> {
        if !cfg!(windows) {
            return Ok(());
        }
        let exe = self.layout.ollama_exe().display().to_string();
        // `ollama serve` spawns `lib\ollama\llama-server.exe` runners that
        // survive their parent and keep the model in VRAM; the first restart
        // on the PC left three of them and the next load failed (5 GB free
        // of 12). Kill everything that runs from our ollama_dir.
        let dir = self
            .layout
            .ollama_dir
            .display()
            .to_string()
            .replace('\'', "''");
        let script = format!(
            "Get-Process ollama,llama-server -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq '{exe}' -or ($_.Path -and $_.Path.StartsWith('{dir}')) }} | Stop-Process -Force"
        );
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        hide_console(&mut cmd);
        let _ = cmd.status().await;
        for _ in 0..40 {
            if !self.is_running().await {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        Err(CoreError::Other(
            "ollama still answers after stopping owned processes".into(),
        ))
    }

    /// `<app_data_dir>/logs/ollama.log` — stdout+stderr of `ollama serve`.
    pub fn log_file(&self) -> std::path::PathBuf {
        self.layout.app_data_dir.join("logs").join("ollama.log")
    }

    fn log_tail(&self, n: usize) -> String {
        let text = std::fs::read_to_string(self.log_file()).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.len().saturating_sub(n);
        lines[start..].join(" | ")
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
        self.models()
            .await
            .map(|m| m.iter().any(|n| normalize_tag(n) == want))
            .unwrap_or(false)
    }

    /// `POST /api/pull` with streaming NDJSON progress.
    pub async fn pull(
        &self,
        tag: &str,
        events: &EventSender,
        cancel: &CancellationToken,
    ) -> Result<()> {
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
                let Ok(text) = std::str::from_utf8(&line) else {
                    continue;
                };
                let Ok(p) = serde_json::from_str::<PullLine>(text.trim()) else {
                    continue;
                };
                if let Some(err) = p.error {
                    return Err(CoreError::Other(format!("ollama pull: {err}")));
                }
                if p.status != last_status {
                    last_status = p.status.clone();
                    emit(
                        events,
                        SetupEvent::Log {
                            line: format!("ollama: {}", p.status),
                        },
                    );
                }
                if let (Some(total), Some(done)) = (p.total, p.completed) {
                    emit(
                        events,
                        SetupEvent::Progress {
                            label: format!("model {tag}"),
                            received: done,
                            total: Some(total),
                        },
                    );
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
            emit(
                events,
                SetupEvent::Progress {
                    label: label.to_string(),
                    received,
                    total,
                },
            );
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, dest).await?;
    emit(
        events,
        SetupEvent::Progress {
            label: label.to_string(),
            received,
            total: Some(total.unwrap_or(received)),
        },
    );
    Ok(())
}

/// Extract a zip archive into `dest`, refusing entries that escape it.
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
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
