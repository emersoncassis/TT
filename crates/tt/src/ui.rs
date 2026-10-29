//! Serves the embedded web UI (single-page app) and the optional custom logo.
use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

use crate::app::Shared;

#[derive(RustEmbed)]
#[folder = "../../ui/"]
struct Assets;

pub async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref().to_string()), (header::CACHE_CONTROL, "no-cache".to_string())], f.data.into_owned()).into_response()
        }
        None => match Assets::get("index.html") {
            Some(f) => ([(header::CONTENT_TYPE, "text/html".to_string())], f.data.into_owned()).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

/// `data/logo.<ext>` (png/svg/jpg/webp) replaces the default logo.
pub async fn logo(State(s): State<Shared>, Path(_): Path<String>) -> Response {
    for (ext, mime) in [("svg", "image/svg+xml"), ("png", "image/png"), ("webp", "image/webp"), ("jpg", "image/jpeg")] {
        if let Ok(bytes) = std::fs::read(s.data_dir.join(format!("logo.{ext}"))) {
            return ([(header::CONTENT_TYPE, mime)], bytes).into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}
