#![allow(dead_code)]

pub mod composer;
pub mod matrix;
pub mod tokens;

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
use whl_api::{ApiLimits, ApiSettings, ApiState, ComposerSettings};
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
    let bytes = axum::body::to_bytes(body, 4 * 1024 * 1024)
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
        export_concurrency: 2,
        storage_cap_bytes: 20 * 1024 * 1024 * 1024,
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
    build_with_users(false, TrustedProxies::default(), None).await
}

pub async fn sso_harness_with(composer: ComposerSettings) -> SsoHarness {
    build_with_users(false, TrustedProxies::default(), Some(composer)).await
}

pub async fn mixed_harness() -> SsoHarness {
    build_with_users(true, TrustedProxies::default(), None).await
}

pub async fn mixed_harness_with(composer: ComposerSettings) -> SsoHarness {
    build_with_users(true, TrustedProxies::default(), Some(composer)).await
}

pub async fn mixed_harness_behind(proxies: TrustedProxies) -> SsoHarness {
    build_with_users(true, proxies, None).await
}

pub async fn mixed_harness_with_api(settings: ApiSettings) -> SsoHarness {
    build_with_settings(
        true,
        TrustedProxies::default(),
        settings,
        Duration::ZERO,
        None,
    )
    .await
}

pub async fn mixed_harness_with_import_cooldown(window: Duration) -> SsoHarness {
    build_with_settings(true, TrustedProxies::default(), api_settings(), window, None).await
}

async fn build_with_users(
    mixed: bool,
    proxies: TrustedProxies,
    composer: Option<ComposerSettings>,
) -> SsoHarness {
    build_with_settings(mixed, proxies, api_settings(), Duration::ZERO, composer).await
}

async fn build_with_settings(
    mixed: bool,
    proxies: TrustedProxies,
    api: ApiSettings,
    import_cooldown: Duration,
    composer: Option<ComposerSettings>,
) -> SsoHarness {
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
    let state = ApiState::new(store.clone(), api)
        .with_access(access.clone())
        .with_import_cooldown(import_cooldown);
    let state = match composer {
        Some(composer) => state.with_composer(composer),
        None => state,
    };
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

    pub fn request_as(
        &self,
        actor: &Actor,
        method: Method,
        uri: &str,
        body: &str,
    ) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, &actor.cookie)
            .header(header::ORIGIN, "https://whl.test")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_owned()))
            .expect("the request is valid");
        let address = SocketAddr::new(composer::PEER.parse().expect("a peer address"), 40000);
        request.extensions_mut().insert(ConnectInfo(address));
        request
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
        tags: Vec::new(),
        verifier: None,
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
    pub last_id: Option<String>,
}

impl SsoHarness {
    pub async fn live_app(&self, recheck: Duration) -> LiveApp {
        let hub = Hub::new(HUB_CAPACITY);
        let mut settings = StreamSettings::new(RETENTION);
        settings.recheck_interval = recheck;
        let live = LiveState::new(self.store.clone(), Arc::clone(&hub), settings)
            .with_authorizer(whl_api::stream_authorizer(&self.access))
            .with_inbox(whl_api::inbox_selector(
                &self.access,
                self.store.clone(),
                RETENTION,
            ));
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
        self.open_uri(actor, &format!("/api/v1/endpoints/{endpoint}/stream"), None)
            .await
    }

    pub async fn open_inbox(
        &self,
        actor: Option<&Actor>,
        query: &str,
        last_event_id: Option<&str>,
    ) -> Sse {
        self.open_uri(
            actor,
            &format!("/api/v1/inbox/stream?{query}"),
            last_event_id,
        )
        .await
    }

    async fn open_uri(&self, actor: Option<&Actor>, uri: &str, last_event_id: Option<&str>) -> Sse {
        let mut builder = Request::builder().uri(uri);
        if let Some(actor) = actor {
            builder = builder.header(header::COOKIE, &actor.cookie);
        }
        if let Some(last_event_id) = last_event_id {
            builder = builder.header("last-event-id", last_event_id);
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
            last_id: None,
        }
    }
}

impl LiveApp {
    pub async fn open_bearer(&self, token: &str, endpoint: &str) -> Sse {
        let request = Request::builder()
            .uri(format!("/api/v1/endpoints/{endpoint}/stream"))
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .expect("the request is valid");
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
            last_id: None,
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
                    if let Some(id) = block.lines().find_map(|line| line.strip_prefix("id: ")) {
                        self.last_id = Some(id.to_owned());
                    }
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

    pub async fn error_body(&mut self) -> Value {
        let mut text = String::new();
        while let Some(chunk) = self.body.next().await {
            text.push_str(&String::from_utf8_lossy(
                &chunk.expect("the chunk is readable"),
            ));
        }
        serde_json::from_str(&text).expect("the error body is JSON")
    }

    pub async fn expect_end_after_bye(&mut self) {
        self.expect_event("bye").await;
        let after = self.next_event().await;
        assert!(after.is_none(), "an event arrived after bye: {after:?}");
    }
}

impl LiveApp {
    pub async fn open_with_headers(&self, uri: &str, headers: &[(&str, &str)]) -> Sse {
        let mut builder = Request::builder().uri(uri);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
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
            last_id: None,
        }
    }
}
