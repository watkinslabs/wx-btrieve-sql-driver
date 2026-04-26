//! UI asset embedder. Rust-embed compiles `ui/dist/` into the binary at
//! build time. The router serves `/` and any unmatched path that isn't
//! under `/api/` from this asset bundle (SPA-style fallback to
//! `index.html`).

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist/"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(content) = Assets::get(path) {
        return ok(path, content.data.into());
    }

    // SPA fallback — anything not /api/* falls through to index.html so
    // client-side routes work on direct navigation / refresh.
    if let Some(content) = Assets::get("index.html") {
        return ok("index.html", content.data.into());
    }

    (StatusCode::NOT_FOUND, "ui not embedded").into_response()
}

fn ok(path: &str, body: Body) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut resp = Response::new(body);
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    resp
}
