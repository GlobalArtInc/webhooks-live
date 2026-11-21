#![allow(dead_code)]

pub mod matrix;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use futures_util::StreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::watch;
use tower::ServiceExt;
use url::Url;
use whl_api::{ApiLimits, ApiSettings, ApiState};
use whl_auth::{AccessControl, AuthSettings, AuthState, OidcClient, SessionLimits};
use whl_core::client_ip::TrustedProxies;
use whl_live::{HUB_CAPACITY, Hub, LiveState, StreamSettings};
use whl_store::{Store, Uuid, VerifiedIdentity};
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
    let state = ApiState::new(store, api_settings());
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

pub const SESSION_COOKIE: &str = "__Host-whl_session";
pub const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

pub fn api_settings() -> ApiSettings {
    ApiSettings {
        public_url: "https://whl.test".to_owned(),
        retention: RETENTION,
        max_captures: 500,
        max_body_bytes: 10 * 1024 * 1024,
        forwarding: true,
        private_forward_targets_allowed: false,
        ring_mode_allowed: true,
    }
}

#[derive(Clone)]
pub struct Actor {
    pub id: Uuid,
    pub email: String,
    pub cookie: String,
}

pub struct SsoHarness {
    pub db: TestDb,
    pub app: Router,
    pub store: Store,
    pub access: AccessControl,
}

pub async fn sso_harness() -> SsoHarness {
    build_with_users(false, TrustedProxies::default()).await
}

pub async fn mixed_harness() -> SsoHarness {
    build_with_users(true, TrustedProxies::default()).await
}

pub async fn mixed_harness_behind(proxies: TrustedProxies) -> SsoHarness {
    build_with_users(true, proxies).await
}

async fn build_with_users(mixed: bool, proxies: TrustedProxies) -> SsoHarness {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let issuer = Url::parse("https://sso.test").expect("the issuer URL parses");
    let public = Url::parse("https://whl.test").expect("the public URL parses");
    let settings = AuthSettings::new(
        OidcClient {
            issuer: &issuer,
            client_id: "client",
            client_secret: "secret",
            scopes: &[],
        },
        &public,
        SessionLimits {
            idle: Duration::from_secs(3600),
            max: Duration::from_secs(7200),
        },
    )
    .expect("the settings are valid")
    .with_provider_name("Test SSO");
    let auth = AuthState::new(store.clone(), settings).expect("the auth state builds");
    let access = if mixed {
        AccessControl::mixed(auth, RETENTION)
    } else {
        AccessControl::sso(auth, RETENTION)
    }
    .with_proxies(proxies);
    let state = ApiState::new(store.clone(), api_settings()).with_access(access.clone());
    SsoHarness {
        db,
        app: whl_api::router(state),
        store,
        access,
    }
}

impl SsoHarness {
    pub async fn actor(&self, tag: &str, email: &str, verified: bool) -> Actor {
        let user = self
            .store
            .upsert_user(&VerifiedIdentity {
                issuer: "https://sso.test".to_owned(),
                subject: format!("subject-{tag}"),
                email: email.to_owned(),
                email_verified: verified,
                display_name: Some(format!("User {tag}")),
            })
            .await
            .expect("the user is stored");
        let token = format!("{tag:0<43}");
        self.store
            .create_session(
                Sha256::digest(token.as_bytes()).as_slice(),
                user.id,
                Duration::from_secs(3600),
            )
            .await
            .expect("the session is stored");
        Actor {
            id: user.id,
            email: email.to_owned(),
            cookie: format!("{SESSION_COOKIE}={token}"),
        }
    }

    pub async fn send_as(
        &self,
        actor: Option<&Actor>,
        method: Method,
        uri: &str,
        body: &str,
    ) -> Reply {
        self.send_from(actor, None, method, uri, body).await
    }

    pub async fn send_from(
        &self,
        actor: Option<&Actor>,
        peer: Option<&str>,
        method: Method,
        uri: &str,
        body: &str,
    ) -> Reply {
        self.send_with(actor, peer, &[], method, uri, body).await
    }

    pub async fn send_with(
        &self,
        actor: Option<&Actor>,
        peer: Option<&str>,
        extra: &[(&str, &str)],
        method: Method,
        uri: &str,
        body: &str,
    ) -> Reply {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(actor) = actor {
            builder = builder.header(header::COOKIE, &actor.cookie);
        }
        for (name, value) in extra {
            builder = builder.header(*name, *value);
        }
        let mut request = if body.is_empty() {
            builder.body(Body::empty())
        } else {
            builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
        }
        .expect("the request is valid");
        if let Some(peer) = peer {
            let address = SocketAddr::new(peer.parse().expect("a valid peer address"), 40000);
            request.extensions_mut().insert(ConnectInfo(address));
        }
        send(&self.app, request).await
    }

    pub async fn anonymous_endpoint(
        &self,
        network: Option<&whl_core::creator_net::CreatorNet>,
    ) -> String {
        self.store
            .create_endpoint_from_network(network, &resolve_defaults(), RETENTION)
            .await
            .expect("the endpoint is stored")
            .id
            .unpack()
            .to_owned()
    }

    pub async fn endpoint_for(&self, owner: &Actor) -> String {
        self.store
            .create_endpoint_owned(Some(owner.id), &resolve_defaults(), RETENTION)
            .await
            .expect("the endpoint is stored")
            .id
            .unpack()
            .to_owned()
    }
}

fn resolve_defaults() -> whl_store::EndpointSettings {
    whl_store::EndpointSettings {
        title: None,
        response: whl_core::response::ResponseConfig::default(),
        templating: Default::default(),
        cors: false,
        overflow: whl_store::Overflow::Refuse,
    }
}

pub const STREAM_WAIT: Duration = Duration::from_secs(10);

pub struct LiveApp {
    pub app: Router,
    _stop: watch::Sender<bool>,
}

pub struct Sse {
    pub status: StatusCode,
    body: axum::body::BodyDataStream,
    buffer: String,
}

impl SsoHarness {
    pub async fn live_app(&self, recheck: Duration) -> LiveApp {
        let hub = Hub::new(HUB_CAPACITY);
        let mut settings = StreamSettings::new(RETENTION);
        settings.recheck_interval = recheck;
        let live = LiveState::new(self.store.clone(), Arc::clone(&hub), settings)
            .with_authorizer(whl_api::stream_authorizer(&self.access));
        let (stop, mut stopped) = watch::channel(false);
        let listener = whl_live::spawn_listener(self.store.clone(), hub, async move {
            let _ = stopped.wait_for(|stop| *stop).await;
        });
        listener
            .wait_listening()
            .await
            .expect("the listener starts listening");
        let app =
            whl_api::guard_routes(whl_live::router(live), &self.access).merge(self.app.clone());
        LiveApp { app, _stop: stop }
    }
}

impl LiveApp {
    pub async fn open(&self, actor: Option<&Actor>, endpoint: &str) -> Sse {
        let mut builder = Request::builder().uri(format!("/api/v1/endpoints/{endpoint}/stream"));
        if let Some(actor) = actor {
            builder = builder.header(header::COOKIE, &actor.cookie);
        }
        let request = builder.body(Body::empty()).expect("the request is valid");
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("the router does not fail");
        let (parts, body) = response.into_parts();
        Sse {
            status: parts.status,
            body: body.into_data_stream(),
            buffer: String::new(),
        }
    }
}

impl Sse {
    pub async fn next_event(&mut self) -> Option<(String, Value)> {
        loop {
            while let Some(end) = self.buffer.find("\n\n") {
                let block: String = self.buffer.drain(..end + 2).collect();
                let name = block
                    .lines()
                    .find_map(|line| line.strip_prefix("event: "))
                    .map(str::to_owned);
                if let Some(name) = name {
                    let data = block
                        .lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .and_then(|data| serde_json::from_str(data).ok())
                        .unwrap_or(Value::Null);
                    return Some((name, data));
                }
            }
            let chunk = tokio::time::timeout(STREAM_WAIT, self.body.next())
                .await
                .expect("the stream delivers or ends in time")?;
            let chunk = chunk.expect("the body chunk is readable");
            self.buffer.push_str(&String::from_utf8_lossy(&chunk));
        }
    }

    pub async fn expect_event(&mut self, name: &str) -> Value {
        let (event, data) = self
            .next_event()
            .await
            .unwrap_or_else(|| panic!("the stream ended while waiting for {name}"));
        assert_eq!(event, name, "{data}");
        data
    }

    pub async fn expect_end_after_bye(&mut self) {
        self.expect_event("bye").await;
        let after = self.next_event().await;
        assert!(after.is_none(), "an event arrived after bye: {after:?}");
    }
}
