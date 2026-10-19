//! The server's web panel: the static build of `src/server/` (`resources/server-ui/`)
//! served next to the control API on 127.0.0.1:8645. The page talks to
//! `/control/v1` with the control token the administrator pastes at login, so
//! the panel adds no new way in.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;

pub const BASE: &str = "/ui";

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                   img-src 'self' data:; font-src 'self' data:; connect-src 'self'; \
                   frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

pub fn router(dir: PathBuf) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::temporary("/ui/") }))
        .route("/ui", get(|| async { Redirect::temporary("/ui/") }))
        .route("/ui/", get(file))
        .route("/ui/{*path}", get(file))
        .with_state(dir)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

/// `/ui/<rel>` → a file inside `dir`; `None` for anything that could leave it.
pub fn resolve(dir: &Path, uri_path: &str) -> Option<PathBuf> {
    let rel = uri_path.strip_prefix(BASE)?.trim_start_matches('/');
    if rel
        .split('/')
        .any(|seg| seg == ".." || seg.contains('\\') || seg.contains(':'))
    {
        return None;
    }
    Some(if rel.is_empty() {
        dir.join("index.html")
    } else {
        dir.join(rel)
    })
}

async fn file(State(dir): State<PathBuf>, uri: Uri) -> Response {
    let Some(mut path) = resolve(&dir, uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Unknown paths are client-side routes of the single page.
    if !path.is_file() {
        path = dir.join("index.html");
    }
    let Ok(body) = tokio::fs::read(&path).await else {
        return (
            StatusCode::NOT_FOUND,
            "web panel is not bundled (resources/server-ui is missing)",
        )
            .into_response();
    };
    let cache = if path.parent().is_some_and(|p| p.ends_with("assets")) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut resp = body.into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type(&path)),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inside_the_folder_only() {
        let d = Path::new("/ui-dir");
        assert_eq!(resolve(d, "/ui/"), Some(d.join("index.html")));
        assert_eq!(resolve(d, "/ui/assets/a.js"), Some(d.join("assets/a.js")));
        assert_eq!(resolve(d, "/ui/../control.token"), None);
        assert_eq!(resolve(d, "/ui/assets/..%2f"), Some(d.join("assets/..%2f")));
        assert_eq!(resolve(d, "/ui/a\\b"), None);
        assert_eq!(resolve(d, "/other"), None);
    }
}
