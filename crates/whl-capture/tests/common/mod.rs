#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::request::Builder;
use axum::http::{HeaderMap, Request};
use sqlx::Row;
use time::OffsetDateTime;
use tower::ServiceExt;
use whl_capture::{CaptureLimits, CaptureSettings, CaptureSource, CaptureState, router};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::IdGenerationError;
use whl_core::limiter::Limiters;
use whl_store::Store;
use whl_testkit::TestDb;

pub const PUBLIC_URL: &str = "https://hooks.example.test";
pub const SENDER: &str = "198.51.100.10:4000";

pub struct Harness {
    pub db: TestDb,
    pub state: CaptureState,
    pub router: Router,
}

pub fn default_limits() -> CaptureLimits {
    CaptureLimits {
        max_body_bytes: 1 << 20,
        max_captures: 500,
        retention: Duration::from_secs(7 * 24 * 3600),
        body_memory_budget: 64 << 20,
        body_read_timeout: Duration::from_secs(30),
        budget_wait: Duration::from_secs(2),
    }
}

impl Harness {
    pub async fn new() -> Self {
        Self::build(default_limits(), Vec::new()).await
    }

    pub async fn with_limits(limits: CaptureLimits) -> Self {
        Self::build(limits, Vec::new()).await
    }

    pub async fn with_proxies(proxies: &[&str]) -> Self {
        let nets = proxies
            .iter()
            .map(|net| net.parse().expect("valid CIDR"))
            .collect();
        Self::build(default_limits(), nets).await
    }

    pub async fn build(limits: CaptureLimits, trusted_proxies: Vec<ipnet::IpNet>) -> Self {
        let db = TestDb::new().await;
        let state = CaptureState::new(
            Store::from_pool(db.pool().clone()),
            CaptureSettings {
                limits,
                trusted_proxies,
                public_url: PUBLIC_URL.to_owned(),
                max_delayed: 2000,
            },
        )
        .expect("settings are valid");
        let router = router(state.clone());
        Self { db, state, router }
    }

    pub fn with_source(mut self, source: Arc<dyn CaptureSource>) -> Self {
        self.state = self.state.with_source(source);
        self.router = router(self.state.clone());
        self
    }

    pub fn with_limiters(mut self, limiters: Limiters) -> Self {
        self.state = self.state.with_limiters(limiters);
        self.router = router(self.state.clone());
        self
    }

    pub async fn endpoint(&self) -> EndpointId {
        self.endpoint_with("refuse").await
    }

    pub async fn endpoint_with(&self, overflow: &str) -> EndpointId {
        let id = EndpointId::generate().expect("entropy is available");
        sqlx::query("INSERT INTO endpoints (id, overflow) VALUES ($1, $2)")
            .bind(id.unpack())
            .bind(overflow)
            .execute(self.db.pool())
            .await
            .expect("endpoint inserts");
        id
    }

    pub async fn send(&self, request: Request<Body>) -> Reply {
        send(&self.router, request).await
    }

    pub async fn get(&self, path: &str) -> Reply {
        self.send(
            request("GET", path)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
    }

    pub async fn post(&self, path: &str, body: &'static str) -> Reply {
        self.send(
            request("POST", path)
                .body(Body::from(body))
                .expect("request builds"),
        )
        .await
    }

    pub async fn rows(&self, endpoint: &EndpointId) -> Vec<Stored> {
        let rows = sqlx::query(
            "SELECT c.seq, c.id::text AS id, c.method, c.url, c.path, c.host, \
             host(c.client_ip) AS client_ip, c.headers::text AS headers, c.query::text AS query, \
             c.form::text AS form, c.content_type, c.body_size, c.stored_bytes, \
             c.status_override, c.response_status, c.received_at, b.data, b.search_text \
             FROM captures c JOIN capture_bodies b ON b.capture_id = c.id \
             WHERE c.endpoint_id = $1 ORDER BY c.seq",
        )
        .bind(endpoint.unpack())
        .fetch_all(self.db.pool())
        .await
        .expect("captures read");
        rows.iter().map(Stored::from_row).collect()
    }

    pub async fn counters(&self, endpoint: &EndpointId) -> Counters {
        let row =
            sqlx::query("SELECT capture_count, bytes_used, last_seq FROM endpoints WHERE id = $1")
                .bind(endpoint.unpack())
                .fetch_one(self.db.pool())
                .await
                .expect("endpoint reads");
        Counters {
            capture_count: row.get("capture_count"),
            bytes_used: row.get("bytes_used"),
            last_seq: row.get("last_seq"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Counters {
    pub capture_count: i32,
    pub bytes_used: i64,
    pub last_seq: i64,
}

pub struct Reply {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: String,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|value| value.to_str().ok())
    }
}

pub fn request(method: &str, path: &str) -> Builder {
    request_from(method, path, SENDER)
}

pub fn request_from(method: &str, path: &str, peer: &str) -> Builder {
    let peer: SocketAddr = peer.parse().expect("valid socket address");
    Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(peer))
}

pub async fn send(router: &Router, request: Request<Body>) -> Reply {
    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router is infallible");
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.expect("body reads");
    Reply {
        status: parts.status.as_u16(),
        headers: parts.headers,
        body: String::from_utf8_lossy(&bytes).into_owned(),
    }
}

#[derive(Debug)]
pub struct Stored {
    pub seq: i64,
    pub id: String,
    pub method: String,
    pub url: String,
    pub path: String,
    pub host: String,
    pub client_ip: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub form: Option<serde_json::Value>,
    pub content_type: Option<String>,
    pub body_size: i32,
    pub stored_bytes: i32,
    pub status_override: Option<i16>,
    pub response_status: i16,
    pub received_at: OffsetDateTime,
    pub body: Vec<u8>,
    pub search_text: String,
}

impl Stored {
    fn from_row(row: &sqlx::postgres::PgRow) -> Self {
        let form: Option<String> = row.get("form");
        Self {
            seq: row.get("seq"),
            id: row.get("id"),
            method: row.get("method"),
            url: row.get("url"),
            path: row.get("path"),
            host: row.get("host"),
            client_ip: row.get("client_ip"),
            headers: pairs(&row.get::<String, _>("headers")),
            query: pairs(&row.get::<String, _>("query")),
            form: form.map(|text| serde_json::from_str(&text).expect("form is JSON")),
            content_type: row.get("content_type"),
            body_size: row.get("body_size"),
            stored_bytes: row.get("stored_bytes"),
            status_override: row.get("status_override"),
            response_status: row.get("response_status"),
            received_at: row.get("received_at"),
            body: row.get("data"),
            search_text: row.get("search_text"),
        }
    }

    pub fn header_values(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
            .collect()
    }
}

fn pairs(text: &str) -> Vec<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(text).expect("pairs are JSON");
    value
        .as_array()
        .expect("pairs are an array")
        .iter()
        .map(|item| {
            (
                item["name"].as_str().expect("name").to_owned(),
                item["value"].as_str().expect("value").to_owned(),
            )
        })
        .collect()
}

pub struct FixedSource {
    pub id: CaptureId,
    pub at: OffsetDateTime,
}

impl CaptureSource for FixedSource {
    fn now(&self) -> OffsetDateTime {
        self.at
    }

    fn capture_id(&self) -> Result<CaptureId, IdGenerationError> {
        Ok(self.id)
    }
}
