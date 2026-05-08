// SPDX-License-Identifier: AGPL-3.0-only

//! Integration test: `edb_web::router()` can be merged with a JSON-RPC
//! `POST /` and `GET /health` router without route conflicts.

use axum::{
    Json, Router,
    body::Body,
    http::Request,
    routing::{get, post},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn fake_rpc(Json(req): Json<Value>) -> Json<Value> {
    Json(json!({ "jsonrpc": "2.0", "id": req.get("id"), "result": "ok" }))
}

async fn fake_health() -> Json<Value> {
    Json(json!({ "status": "healthy" }))
}

fn merged_app() -> Router {
    Router::new()
        .route("/", post(fake_rpc))
        .route("/health", get(fake_health))
        .merge(edb_web::router())
}

#[tokio::test]
async fn post_root_hits_rpc_handler() {
    let app = merged_app();
    let body = json!({"jsonrpc":"2.0","method":"foo","id":1}).to_string();
    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["result"], "ok");
}

#[tokio::test]
async fn get_root_hits_static_handler() {
    let app = merged_app();
    let res = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();

    assert_eq!(res.status(), 200);
    let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.starts_with("text/html"), "ct was {ct}");
}

#[tokio::test]
async fn get_health_hits_engine_handler() {
    let app = merged_app();
    let res =
        app.oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap()).await.unwrap();

    assert_eq!(res.status(), 200);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["status"], "healthy");
}
