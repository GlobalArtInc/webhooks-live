#![allow(dead_code)]

pub mod mock_issuer;

use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use axum::routing::{any, get};
use mock_issuer::{CLIENT_ID, CLIENT_SECRET, MockIssuer};
use openidconnect::reqwest;
use serde_json::{Value, json};
use tower::ServiceExt;
use url::Url;
use whl_auth::{AuthSettings, AuthState, CurrentUser, OidcClient, SessionLimits};
use whl_store::Store;
use whl_testkit::TestDb;

pub const PUBLIC_URL: &str = "https://whl.test";
pub const SESSION_COOKIE: &str = "__Host-whl_session";
pub const LOGIN_COOKIE_PREFIX: &str = "__Host-whl_login_";

pub struct Options {
    pub public_url: &'static str,
    pub idle: Duration,
    pub max: Duration,
    pub establish: bool,
    pub max_pending_logins: Option<u32>,
    pub max_pending_per_network: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            public_url: PUBLIC_URL,
            idle: Duration::from_secs(3600),
            max: Duration::from_secs(7200),
            establish: true,
            max_pending_logins: None,
            max_pending_per_network: None,
        }
    }
}

pub struct Harness {
    pub db: TestDb,
    pub issuer: MockIssuer,
    pub auth: AuthState,
    pub store: Store,
    pub app: Router,
    http: reqwest::Client,
}

pub async fn harness() -> Harness {
    harness_with(Options::default()).await
}

pub async fn harness_with(options: Options) -> Harness {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let issuer = MockIssuer::start().await;
    let issuer_url = issuer.issuer_url();
    let scopes = ["openid", "email", "profile"].map(str::to_owned);
    let settings = AuthSettings::new(
        OidcClient {
            issuer: &issuer_url,
            client_id: CLIENT_ID,
            client_secret: CLIENT_SECRET,
            scopes: &scopes,
        },
        &Url::parse(options.public_url).expect("the public URL parses"),
        SessionLimits {
            idle: options.idle,
            max: options.max,
        },
    )
    .expect("the settings are valid");
    let settings = match options.max_pending_logins {
        Some(maximum) => settings
            .with_max_pending_logins(maximum)
            .expect("the maximum is valid"),
        None => settings,
    };
    let settings = match options.max_pending_per_network {
        Some(maximum) => settings
            .with_max_pending_logins_per_network(maximum)
            .expect("the maximum is valid"),
        None => settings,
    };
    let auth = if options.establish {
        AuthState::discover(store.clone(), settings)
            .await
            .expect("discovery against the mock issuer succeeds")
    } else {
        AuthState::new(store.clone(), settings).expect("the auth state builds")
    };
    let app = whl_auth::router(auth.clone()).merge(probe_routes(&auth));
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("the HTTP client builds");
    Harness {
        db,
        issuer,
        auth,
        store,
        app,
        http,
    }
}

fn probe_routes(auth: &AuthState) -> Router {
    Router::new()
        .route("/api/v1/whoami", get(whoami))
        .route("/api/v1/maybe", get(maybe))
        .route("/api/v1/change", any(change))
        .with_state(auth.clone())
        .layer(auth.origin_check())
}

async fn whoami(user: CurrentUser) -> axum::Json<Value> {
    axum::Json(json!({
        "id": user.unpack().id.to_string(),
        "email": user.unpack().email,
        "name": user.unpack().display_name,
    }))
}

async fn maybe(user: Option<CurrentUser>) -> axum::Json<Value> {
    axum::Json(json!({ "signed_in": user.is_some() }))
}

async fn change() -> StatusCode {
    StatusCode::NO_CONTENT
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

    pub fn location(&self) -> &str {
        self.headers
            .get(header::LOCATION)
            .expect("a location header")
            .to_str()
            .expect("the location is text")
    }

    pub fn set_cookies(&self) -> Vec<String> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|value| value.to_str().expect("cookie text").to_owned())
            .collect()
    }

    pub fn set_cookie_starting(&self, prefix: &str) -> Option<String> {
        self.set_cookies()
            .into_iter()
            .find(|cookie| cookie.starts_with(prefix))
    }

    pub fn set_cookie(&self, name: &str) -> Option<String> {
        let prefix = format!("{name}=");
        self.set_cookies()
            .into_iter()
            .find(|cookie| cookie.starts_with(&prefix))
    }
}

#[derive(Default, Clone)]
pub struct Jar(Vec<(String, String)>);

impl Jar {
    pub fn absorb(&mut self, reply: &Reply) {
        for cookie in reply.set_cookies() {
            let mut parts = cookie.split("; ");
            let Some((name, value)) = parts.next().and_then(|pair| pair.split_once('=')) else {
                continue;
            };
            let removed = value.is_empty() || parts.any(|part| part == "Max-Age=0");
            self.0.retain(|(existing, _)| existing != name);
            if !removed {
                self.0.push((name.to_owned(), value.to_owned()));
            }
        }
    }

    pub fn set(&mut self, name: &str, value: &str) {
        self.0.retain(|(existing, _)| existing != name);
        self.0.push((name.to_owned(), value.to_owned()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(existing, _)| existing == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn names_starting(&self, prefix: &str) -> Vec<String> {
        self.0
            .iter()
            .filter(|(name, _)| name.starts_with(prefix))
            .map(|(name, _)| name.clone())
            .collect()
    }

    pub fn remove(&mut self, name: &str) {
        self.0.retain(|(existing, _)| existing != name);
    }

    pub fn header(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        Some(
            self.0
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
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

pub fn request(
    method: Method,
    uri: &str,
    jar: &Jar,
    headers: &[(header::HeaderName, &str)],
) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(cookie) = jar.header() {
        builder = builder.header(header::COOKIE, cookie);
    }
    for (name, value) in headers {
        builder = builder.header(name.clone(), *value);
    }
    builder.body(Body::empty()).expect("the request is valid")
}

pub struct Begun {
    pub reply: Reply,
    pub jar: Jar,
}

impl Harness {
    pub async fn get(&self, uri: &str, jar: &Jar) -> Reply {
        send(&self.app, request(Method::GET, uri, jar, &[])).await
    }

    pub async fn begin(&self, return_to: Option<&str>) -> Begun {
        self.begin_with(return_to, Jar::default()).await
    }

    pub async fn begin_from(&self, peer: &str) -> Reply {
        let mut request = request(Method::GET, "/auth/login", &Jar::default(), &[]);
        let address = std::net::SocketAddr::new(peer.parse().expect("a valid peer"), 40000);
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(address));
        send(&self.app, request).await
    }

    pub async fn begin_with(&self, return_to: Option<&str>, mut jar: Jar) -> Begun {
        let uri = match return_to {
            Some(target) => format!(
                "/auth/login?return_to={}",
                url::form_urlencoded::byte_serialize(target.as_bytes()).collect::<String>()
            ),
            None => "/auth/login".to_owned(),
        };
        let reply = self.get(&uri, &jar).await;
        jar.absorb(&reply);
        Begun { reply, jar }
    }

    pub async fn authorize(&self, location: &str) -> String {
        let response = self
            .http
            .get(location)
            .send()
            .await
            .expect("the mock issuer answers");
        assert_eq!(response.status().as_u16(), 302, "the issuer redirects back");
        let target = response
            .headers()
            .get(header::LOCATION)
            .expect("a location header")
            .to_str()
            .expect("the location is text")
            .to_owned();
        let target = Url::parse(&target).expect("the callback URL parses");
        format!("{}?{}", target.path(), target.query().unwrap_or_default())
    }

    pub async fn callback(&self, uri: &str, jar: &Jar) -> Reply {
        self.get(uri, jar).await
    }

    pub async fn sign_in(&self, return_to: Option<&str>) -> (Reply, Jar) {
        self.sign_in_with(return_to, Jar::default()).await
    }

    pub async fn sign_in_with(&self, return_to: Option<&str>, jar: Jar) -> (Reply, Jar) {
        let begun = self.begin_with(return_to, jar).await;
        assert_eq!(begun.reply.status, StatusCode::FOUND);
        let uri = self.authorize(begun.reply.location()).await;
        let reply = self.callback(&uri, &begun.jar).await;
        let mut jar = begun.jar;
        jar.absorb(&reply);
        (reply, jar)
    }

    pub async fn count(&self, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(self.db.pool())
            .await
            .expect("the count query runs")
    }
}

pub fn assert_login_error(reply: &Reply, code: &str) {
    assert_eq!(reply.status, StatusCode::FOUND);
    assert_eq!(reply.location(), format!("/?login_error={code}"));
    assert!(
        reply.set_cookie(SESSION_COOKIE).is_none(),
        "a failed login sets no session cookie"
    );
}
