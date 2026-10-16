//! Local control API: an HTTP server on loopback so the shell can be driven
//! without its window — status, setup, gateway, Hermes CLI, config files and
//! the application log. Built for remote administration over an SSH tunnel
//! (`tools/hermesctl.sh`); the port is never exposed on the network.
//!
//! Auth: `Authorization: Bearer <token>`. The token is created on first start
//! and stored in `<app_data_dir>/control.token`. `GET /control/v1/health` is
//! the only unauthenticated route.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bootstrap::Pin;
use crate::chat::{ChatMessage, SessionSummary};
use crate::cli::Output;
use crate::setup::{SetupOptions, SetupPlan};
use crate::status::StatusSnapshot;
use crate::supervisor::{Stamped, Supervisor};
use crate::supervisor_chat::ChatStamped;
use crate::{CoreError, Layout, Result};

pub const DEFAULT_PORT: u16 = 8643;
pub const BASE_PATH: &str = "/control/v1";

#[derive(Debug, Clone)]
pub struct ControlConfig {
    pub port: u16,
    pub token: String,
    pub app_name: String,
    pub app_version: String,
    pub log_dir: PathBuf,
}

struct Ctl {
    sup: Arc<Supervisor>,
    cfg: ControlConfig,
}

pub fn token_path(layout: &Layout) -> PathBuf {
    layout.app_data_dir.join("control.token")
}

/// Read the control token, generating and storing one on first use.
pub fn token(layout: &Layout) -> Result<String> {
    let path = token_path(layout);
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim().to_string();
        if s.len() >= 32 {
            return Ok(s);
        }
    }
    let t = crate::secrets::generate_key()?;
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, &t)?;
    Ok(t)
}

/// `HERMES_CONTROL_PORT` or [`DEFAULT_PORT`].
pub fn port_from_env() -> u16 {
    std::env::var("HERMES_CONTROL_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

pub fn router(sup: Arc<Supervisor>, cfg: ControlConfig) -> Router {
    let ctl = Arc::new(Ctl { sup, cfg });
    let protected = Router::new()
        .route("/info", get(info))
        .route("/status", get(status))
        .route("/setup/plan", get(setup_plan))
        .route("/setup/start", post(setup_start))
        .route("/setup/cancel", post(setup_cancel))
        .route("/setup/events", get(setup_events))
        .route("/gateway/start", post(gateway_start))
        .route("/gateway/stop", post(gateway_stop))
        .route("/hermes/cli", post(hermes_cli))
        .route("/hermes/config", get(config_get).put(config_put))
        .route("/hermes/env", get(env_get))
        .route("/logs", get(logs))
        .route(
            "/chat/sessions",
            get(chat_sessions).post(chat_create_session),
        )
        .route(
            "/chat/sessions/{id}",
            axum::routing::delete(chat_delete_session),
        )
        .route("/chat/sessions/{id}/messages", get(chat_messages))
        .route("/chat/send", post(chat_send))
        .route("/chat/stop", post(chat_stop))
        .route("/chat/approve", post(chat_approve))
        .route("/chat/events", get(chat_events))
        .route("/update/token", axum::routing::put(update_token))
        .layer(middleware::from_fn_with_state(ctl.clone(), auth));
    let api = Router::new().route("/health", get(health)).merge(protected);
    Router::new().nest(BASE_PATH, api).with_state(ctl)
}

/// Bind `127.0.0.1:<port>` and serve until the process exits.
pub async fn serve(sup: Arc<Supervisor>, cfg: ControlConfig) -> Result<()> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], cfg.port));
    let app = router(sup, cfg);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    log::info!("control api listening on http://{addr}{BASE_PATH}");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------- errors --

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<CoreError> for ApiError {
    fn from(e: CoreError) -> Self {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

type ApiResult<T> = std::result::Result<T, ApiError>;

fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

// ------------------------------------------------------------------ auth --

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn auth(State(ctl): State<Arc<Ctl>>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| constant_time_eq(t.trim().as_bytes(), ctl.cfg.token.as_bytes()))
        .unwrap_or(false);
    if !ok {
        return ApiError(
            StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token".into(),
        )
        .into_response();
    }
    next.run(req).await
}

// -------------------------------------------------------------- handlers --

#[derive(Serialize)]
struct Info {
    name: String,
    version: String,
    pin: Pin,
    hermes_home: String,
    install_dir: String,
    hermes_cli: String,
    app_data_dir: String,
    ollama_dir: String,
    models_dir: String,
    log_dir: String,
    hermes_installed: bool,
    setup_running: bool,
    control_port: u16,
}

async fn health(State(ctl): State<Arc<Ctl>>) -> Json<Value> {
    Json(json!({ "ok": true, "name": ctl.cfg.app_name, "version": ctl.cfg.app_version }))
}

async fn info(State(ctl): State<Arc<Ctl>>) -> Json<Info> {
    let l = &ctl.sup.ctx.layout;
    let s = |p: &Path| p.display().to_string();
    Json(Info {
        name: ctl.cfg.app_name.clone(),
        version: ctl.cfg.app_version.clone(),
        pin: ctl.sup.ctx.pin.clone(),
        hermes_home: s(&l.hermes_home),
        install_dir: s(&l.install_dir),
        hermes_cli: s(&l.hermes_cli()),
        app_data_dir: s(&l.app_data_dir),
        ollama_dir: s(&l.ollama_dir),
        models_dir: s(&l.models_dir),
        log_dir: s(&ctl.cfg.log_dir),
        hermes_installed: l.bootstrap_marker().exists(),
        setup_running: ctl.sup.setup_running(),
        control_port: ctl.cfg.port,
    })
}

async fn status(State(ctl): State<Arc<Ctl>>) -> ApiResult<Json<StatusSnapshot>> {
    Ok(Json(ctl.sup.status().await?))
}

async fn setup_plan(State(ctl): State<Arc<Ctl>>) -> Json<SetupPlan> {
    Json(ctl.sup.plan())
}

async fn setup_start(
    State(ctl): State<Arc<Ctl>>,
    Json(opts): Json<SetupOptions>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    if !opts.consent {
        return Err(bad_request("consent must be true to download anything"));
    }
    let fut = ctl
        .sup
        .start_setup(opts)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    tokio::spawn(fut);
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "started": true, "last_seq": ctl.sup.last_seq() })),
    ))
}

async fn setup_cancel(State(ctl): State<Arc<Ctl>>) -> Json<Value> {
    ctl.sup.cancel_setup();
    Json(json!({ "ok": true, "setup_running": ctl.sup.setup_running() }))
}

fn default_limit() -> usize {
    500
}

#[derive(Deserialize)]
struct Since {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Serialize)]
struct Events {
    events: Vec<Stamped>,
    last_seq: u64,
    setup_running: bool,
}

async fn setup_events(State(ctl): State<Arc<Ctl>>, Query(q): Query<Since>) -> Json<Events> {
    Json(Events {
        events: ctl.sup.events_since(q.after, q.limit.min(5000)),
        last_seq: ctl.sup.last_seq(),
        setup_running: ctl.sup.setup_running(),
    })
}

async fn gateway_start(State(ctl): State<Arc<Ctl>>) -> ApiResult<Json<Value>> {
    ctl.sup.gateway_start().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn gateway_stop(State(ctl): State<Arc<Ctl>>) -> ApiResult<Json<Value>> {
    ctl.sup.gateway_stop().await?;
    Ok(Json(json!({ "ok": true })))
}

fn default_timeout() -> u64 {
    600
}

#[derive(Deserialize)]
struct CliRequest {
    args: Vec<String>,
    #[serde(default = "default_timeout")]
    timeout_secs: u64,
}

async fn hermes_cli(
    State(ctl): State<Arc<Ctl>>,
    Json(req): Json<CliRequest>,
) -> ApiResult<Json<Output>> {
    if req.args.is_empty() {
        return Err(bad_request("args must not be empty"));
    }
    let timeout = Duration::from_secs(req.timeout_secs.clamp(1, 3600));
    Ok(Json(ctl.sup.hermes_cli(&req.args, timeout).await?))
}

fn read_text(path: &Path) -> ApiResult<String> {
    if !path.exists() {
        return Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("{} does not exist", path.display()),
        ));
    }
    Ok(std::fs::read_to_string(path).map_err(CoreError::from)?)
}

/// Write `content`, keeping the previous version as `<file>.bak`.
pub fn write_with_backup(path: &Path, content: &str) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    if path.exists() {
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        std::fs::copy(path, PathBuf::from(bak))?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

async fn config_get(State(ctl): State<Arc<Ctl>>) -> ApiResult<String> {
    read_text(&ctl.sup.ctx.layout.config_file())
}

async fn config_put(State(ctl): State<Arc<Ctl>>, body: String) -> ApiResult<Json<Value>> {
    if body.trim().is_empty() {
        return Err(bad_request("refusing to write an empty config.yaml"));
    }
    let path = ctl.sup.ctx.layout.config_file();
    write_with_backup(&path, &body)?;
    Ok(Json(
        json!({ "ok": true, "path": path.display().to_string(), "bytes": body.len() }),
    ))
}

/// Hide values of keys that look like credentials (`KEY`, `TOKEN`, `SECRET`,
/// `PASSWORD`); everything else is returned as is.
pub fn mask_env(text: &str) -> String {
    text.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') || !trimmed.contains('=') {
                return line.to_string();
            }
            let (key, _) = trimmed.split_once('=').unwrap_or((trimmed, ""));
            let upper = key.trim().to_ascii_uppercase();
            if ["KEY", "TOKEN", "SECRET", "PASSWORD"]
                .iter()
                .any(|m| upper.contains(m))
            {
                format!("{}=***", key.trim())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn env_get(State(ctl): State<Arc<Ctl>>) -> ApiResult<String> {
    Ok(mask_env(&read_text(&ctl.sup.ctx.layout.env_file())?))
}

fn default_tail() -> usize {
    200
}

#[derive(Deserialize)]
struct Tail {
    #[serde(default = "default_tail")]
    tail: usize,
    /// Explicit file name inside the log dir; default: the newest `*.log`.
    file: Option<String>,
}

/// Newest `*.log` in `dir` by modification time.
pub fn newest_log(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
        if best.as_ref().is_none_or(|(t, _)| modified > *t) {
            best = Some((modified, path));
        }
    }
    best.map(|(_, p)| p)
}

pub fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

async fn logs(State(ctl): State<Arc<Ctl>>, Query(q): Query<Tail>) -> ApiResult<String> {
    let dir = &ctl.cfg.log_dir;
    let path = match q.file {
        Some(name) => {
            if name.contains(['/', '\\']) || name.contains("..") {
                return Err(bad_request("file must be a plain name inside the log dir"));
            }
            dir.join(name)
        }
        None => newest_log(dir).ok_or_else(|| {
            ApiError(
                StatusCode::NOT_FOUND,
                format!("no *.log in {}", dir.display()),
            )
        })?,
    };
    Ok(tail_lines(&read_text(&path)?, q.tail.clamp(1, 20_000)))
}

// ------------------------------------------------------------------ chat --

async fn chat_sessions(State(ctl): State<Arc<Ctl>>) -> ApiResult<Json<Vec<SessionSummary>>> {
    Ok(Json(ctl.sup.chat_sessions().await?))
}

#[derive(Deserialize, Default)]
struct CreateSession {
    title: Option<String>,
}

async fn chat_create_session(
    State(ctl): State<Arc<Ctl>>,
    body: Option<Json<CreateSession>>,
) -> ApiResult<(StatusCode, Json<SessionSummary>)> {
    let title = body.and_then(|Json(b)| b.title);
    let s = ctl.sup.chat_create_session(title.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(s)))
}

async fn chat_delete_session(
    State(ctl): State<Arc<Ctl>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<Value>> {
    ctl.sup.chat_delete_session(&id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn chat_messages(
    State(ctl): State<Arc<Ctl>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> ApiResult<Json<Vec<ChatMessage>>> {
    Ok(Json(ctl.sup.chat_messages(&id).await?))
}

#[derive(Deserialize)]
struct SendRequest {
    session_id: String,
    message: String,
}

async fn chat_send(
    State(ctl): State<Arc<Ctl>>,
    Json(req): Json<SendRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let fut = ctl
        .sup
        .chat_send(&req.session_id, &req.message)
        .map_err(|e| ApiError(StatusCode::CONFLICT, e.to_string()))?;
    tokio::spawn(fut);
    Ok((
        StatusCode::ACCEPTED,
        Json(
            json!({ "accepted": true, "session_id": req.session_id, "last_seq": ctl.sup.chat_last_seq() }),
        ),
    ))
}

#[derive(Deserialize)]
struct StopRequest {
    session_id: String,
}

async fn chat_stop(
    State(ctl): State<Arc<Ctl>>,
    Json(req): Json<StopRequest>,
) -> ApiResult<Json<Value>> {
    ctl.sup.chat_stop(&req.session_id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ApproveRequest {
    run_id: String,
    choice: String,
}

async fn chat_approve(
    State(ctl): State<Arc<Ctl>>,
    Json(req): Json<ApproveRequest>,
) -> ApiResult<Json<Value>> {
    ctl.sup
        .chat_approve(&req.run_id, &req.choice)
        .await
        .map_err(|e| bad_request(e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct TokenRequest {
    /// Read-only GitHub token for the private releases; empty clears it.
    token: String,
}

/// Store the token the in-app updater uses (keyring, file fallback).
async fn update_token(
    State(ctl): State<Arc<Ctl>>,
    Json(req): Json<TokenRequest>,
) -> ApiResult<Json<Value>> {
    crate::secrets::set_github_token(&ctl.sup.ctx.layout, &req.token)?;
    Ok(Json(
        json!({ "ok": true, "configured": !req.token.trim().is_empty() }),
    ))
}

#[derive(Serialize)]
struct ChatEvents {
    events: Vec<ChatStamped>,
    last_seq: u64,
}

async fn chat_events(State(ctl): State<Arc<Ctl>>, Query(q): Query<Since>) -> Json<ChatEvents> {
    Json(ChatEvents {
        events: ctl.sup.chat_events_since(q.after, q.limit.min(5000)),
        last_seq: ctl.sup.chat_last_seq(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_compares_whole_slices() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }

    #[test]
    fn mask_env_hides_credentials_only() {
        let text =
            "# comment\nAPI_SERVER_KEY=secret\nHERMES_API_TIMEOUT=1800\nMY_TOKEN = x\nplain line";
        let masked = mask_env(text);
        assert!(masked.contains("API_SERVER_KEY=***"));
        assert!(masked.contains("MY_TOKEN=***"));
        assert!(masked.contains("HERMES_API_TIMEOUT=1800"));
        assert!(masked.contains("# comment"));
        assert!(masked.contains("plain line"));
        assert!(!masked.contains("secret"));
    }

    #[test]
    fn tail_lines_keeps_last_n() {
        assert_eq!(tail_lines("a\nb\nc\n", 2), "b\nc");
        assert_eq!(tail_lines("a\nb", 10), "a\nb");
        assert_eq!(tail_lines("", 3), "");
    }

    #[test]
    fn token_is_created_once_and_reused() {
        let root = std::env::temp_dir().join(format!("hermes-ctl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let layout = Layout::from_roots(&root, &root.join("data"));
        let a = token(&layout).unwrap();
        let b = token(&layout).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(token_path(&layout).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_with_backup_keeps_previous_version() {
        let root = std::env::temp_dir().join(format!("hermes-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let file = root.join("config.yaml");
        write_with_backup(&file, "one").unwrap();
        write_with_backup(&file, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "two");
        assert_eq!(
            std::fs::read_to_string(root.join("config.yaml.bak")).unwrap(),
            "one"
        );
        assert_eq!(newest_log(&root), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn router_rejects_missing_token_and_serves_health() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let layout = Layout::from_roots(Path::new("/lad"), Path::new("/data"));
        let sup = Supervisor::new(Arc::new(crate::setup::SetupContext {
            layout,
            script: PathBuf::from("install.ps1"),
            pin: Pin {
                version: "v0".into(),
                commit: "abc".into(),
                branch: "main".into(),
            },
        }));
        let app = router(
            sup,
            ControlConfig {
                port: 1,
                token: "t0ken".into(),
                app_name: "Hermes Assistant".into(),
                app_version: "0.0.0".into(),
                log_dir: PathBuf::from("/nonexistent"),
            },
        );
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/control/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/control/v1/info")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/control/v1/setup/plan")
                    .header("authorization", "Bearer t0ken")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}
