// EDB - Ethereum Debugger
// Copyright (C) 2024 Zhuo Zhang and Wuqi Zhang
//
// SPDX-License-Identifier: AGPL-3.0-only

//! Browser-based debugger UI for EDB.
//!
//! Exposes [`router`] which returns an Axum [`axum::Router`] that serves the
//! embedded React SPA bundled at compile time from `frontend/dist/`. Mount it
//! alongside the engine's JSON-RPC routes via `Router::merge`.

use axum::{
    Router,
    body::Body,
    http::{Response, StatusCode, header},
    response::IntoResponse,
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "frontend/dist/"]
struct Assets;

/// Build the static-file router for the embedded SPA.
///
/// Routes:
/// - `GET /` → `index.html`
/// - `GET /<path>` → embedded asset if present, else SPA fallback to `index.html`
///
/// Does not register any POST handlers, so merging this router with the
/// engine's JSON-RPC `POST /` route is safe.
///
/// # Panics
///
/// Panics on first request if no assets were embedded at compile time (e.g.
/// the build was run with `EDB_SKIP_WEB_BUILD=1` and the placeholder is
/// missing).
pub fn router() -> Router {
    if Assets::iter().count() == 0 {
        panic!(
            "edb-web has no embedded assets — rebuild without EDB_SKIP_WEB_BUILD=1 and ensure `bun` is on PATH"
        );
    }
    Router::new()
        .route("/", axum::routing::get(serve_index))
        .fallback(axum::routing::get(serve_asset))
}

async fn serve_index() -> impl IntoResponse {
    serve_path("index.html")
}

async fn serve_asset(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    serve_path(path)
}

fn serve_path(path: &str) -> Response<Body> {
    let candidate = if path.is_empty() { "index.html" } else { path };
    if let Some(file) = Assets::get(candidate) {
        let mime = mime_guess::from_path(candidate).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(file.data.into_owned()))
            .unwrap();
    }
    // SPA fallback: serve index.html for unknown GET paths.
    if let Some(index) = Assets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(index.data.into_owned()))
            .unwrap();
    }
    StatusCode::NOT_FOUND.into_response()
}
