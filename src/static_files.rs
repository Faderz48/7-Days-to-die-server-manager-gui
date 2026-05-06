//! Embed the front-end assets into the binary at compile time so the
//! manager ships as a single executable. Files live in `static/` next
//! to `src/` in the repo.

use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

use crate::state::AppState;
use std::sync::Arc;
use tokio::sync::RwLock;

const INDEX_HTML: &str = include_str!("../static/index.html");
const STYLE_CSS: &str = include_str!("../static/style.css");
const APP_JS: &str = include_str!("../static/app.js");
const FAVICON_SVG: &str = include_str!("../assets/icon.svg");
const FAVICON_ICO: &[u8] = include_bytes!("../assets/icon.ico");

pub fn routes() -> Router<Arc<RwLock<AppState>>> {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/style.css", get(style))
        .route("/app.js", get(app_js))
        .route("/favicon.svg", get(favicon_svg))
        .route("/favicon.ico", get(favicon_ico))
}

fn cached(ct: &'static str, body: &'static str) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    // Avoid aggressive caching during development; flip to immutable in
    // production if you wire up content hashing.
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, must-revalidate"),
    );
    (StatusCode::OK, headers, body)
}

fn cached_bytes(ct: &'static str, body: &'static [u8]) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    (StatusCode::OK, headers, body)
}

async fn index() -> impl IntoResponse {
    cached("text/html; charset=utf-8", INDEX_HTML)
}
async fn style() -> impl IntoResponse {
    cached("text/css; charset=utf-8", STYLE_CSS)
}
async fn app_js() -> impl IntoResponse {
    cached("application/javascript; charset=utf-8", APP_JS)
}
async fn favicon_svg() -> impl IntoResponse {
    cached("image/svg+xml", FAVICON_SVG)
}
async fn favicon_ico() -> impl IntoResponse {
    cached_bytes("image/x-icon", FAVICON_ICO)
}
