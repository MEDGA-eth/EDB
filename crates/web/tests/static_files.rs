// EDB - Ethereum Debugger
// SPDX-License-Identifier: AGPL-3.0-only

//! Integration tests for `edb_web::router()` static-file behavior.

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn root_returns_index_html() {
    let app = edb_web::router();
    let res = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();

    assert_eq!(res.status(), 200);
    let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.starts_with("text/html"), "content-type was {ct}");
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = std::str::from_utf8(&bytes).unwrap();
    assert!(body.contains("<div id=\"root\">"), "body did not contain root div");
}

#[tokio::test]
async fn unknown_path_falls_back_to_index() {
    let app = edb_web::router();
    let res = app
        .oneshot(Request::builder().uri("/some/spa/route").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = std::str::from_utf8(&bytes).unwrap();
    assert!(body.contains("<div id=\"root\">"));
}
