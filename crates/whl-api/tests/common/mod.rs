#![allow(dead_code)]

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use serde_json::Value;
use tower::ServiceExt;
use whl_api::{ApiLimits, ApiSettings, ApiState};
use whl_store::Store;
use whl_testkit::TestDb;

pub const UNKNOWN_ID: &str = "000000000000000000000000";

pub struct Harness {
    pub db: TestDb,
    pub app: Router,
}

pub async fn harness() -> Harness {
    build(None).await
}

pub async fn harness_with_limits(limits: ApiLimits) -> Harness {
    build(Some(limits)).await
}

async fn build(limits: Option<ApiLimits>) -> Harness {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let settings = ApiSettings {
        public_url: "https://whl.test".to_owned(),
        retention: Duration::from_secs(7 * 24 * 3600),
        max_captures: 500,
        max_body_bytes: 10 * 1024 * 1024,
        forwarding: true,
        private_forward_targets_allowed: false,
        ring_mode_allowed: true,
    };
    let state = ApiState::new(store, settings);
    let state = match limits {
        Some(limits) => state.with_limits(limits),
        None => state,
    };
    let app = whl_api::router(state);
    Harness { db, app }
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }
}

pub async fn send(app: &Router, request: Request<Body>) -> Reply {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("the router does not fail");
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, 1024 * 1024)
        .await
        .expect("the body is readable");
    Reply {
        status: parts.status,
        headers: parts.headers,
        body: bytes.to_vec(),
    }
}

pub fn request(method: Method, uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .expect("the request is valid")
}

pub fn bodiless(method: Method, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("the request is valid")
}

pub async fn create(app: &Router, body: &str) -> Value {
    let reply = send(app, request(Method::POST, "/api/v1/endpoints", body)).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    reply.json()
}

pub fn field_codes(error: &Value) -> Vec<(String, String)> {
    error["error"]["fields"]
        .as_array()
        .expect("fields is an array")
        .iter()
        .map(|field| {
            (
                field["field"].as_str().expect("field path").to_owned(),
                field["code"].as_str().expect("field code").to_owned(),
            )
        })
        .collect()
}
