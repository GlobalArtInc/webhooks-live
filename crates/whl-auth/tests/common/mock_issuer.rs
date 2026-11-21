use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use url::{Url, form_urlencoded};
use whl_core::entropy::random_bytes;

pub const CLIENT_ID: &str = "whl-client";
pub const CLIENT_SECRET: &str = "whl-client-secret";
const KEY_ID: &str = "test-key-1";

#[derive(Clone)]
pub struct Behaviour {
    pub subject: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub issuer_claim: Option<String>,
    pub audience_claim: Option<String>,
    pub nonce_claim: Option<String>,
    pub expires_in_seconds: i64,
    pub bad_signature: bool,
    pub tamper_state: bool,
    pub omit_id_token: bool,
    pub reject_token_request: bool,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            subject: "subject-1".to_owned(),
            email: Some("user@example.test".to_owned()),
            email_verified: Some(true),
            name: Some("Test User".to_owned()),
            issuer_claim: None,
            audience_claim: None,
            nonce_claim: None,
            expires_in_seconds: 300,
            bad_signature: false,
            tamper_state: false,
            omit_id_token: false,
            reject_token_request: false,
        }
    }
}

#[derive(Clone)]
pub struct AuthorizeRequest {
    pub client_id: String,
    pub redirect_uri: String,
    pub scope: String,
    pub state: String,
    pub nonce: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
}

#[derive(Clone)]
pub struct TokenRequest {
    pub code_verifier: String,
    pub redirect_uri: String,
    pub basic_credentials: Option<String>,
}

struct Issued {
    request: AuthorizeRequest,
}

#[derive(Default)]
struct Inner {
    behaviour: Behaviour,
    issued: HashMap<String, Issued>,
    authorize_requests: Vec<AuthorizeRequest>,
    token_requests: Vec<TokenRequest>,
    discovery_hits: usize,
}

struct Shared {
    base: String,
    key: SigningKey,
    foreign_key: SigningKey,
    inner: Mutex<Inner>,
}

#[derive(Clone)]
pub struct MockIssuer {
    shared: Arc<Shared>,
}

impl MockIssuer {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("the mock issuer binds");
        let address: SocketAddr = listener.local_addr().expect("the listener has an address");
        let shared = Arc::new(Shared {
            base: format!("http://{address}"),
            key: fresh_key(),
            foreign_key: fresh_key(),
            inner: Mutex::new(Inner::default()),
        });
        let app = Router::new()
            .route("/.well-known/openid-configuration", get(discovery))
            .route("/jwks", get(jwks))
            .route("/authorize", get(authorize))
            .route("/token", post(token))
            .with_state(Arc::clone(&shared));
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self { shared }
    }

    pub fn issuer_url(&self) -> Url {
        Url::parse(&self.shared.base).expect("the issuer URL parses")
    }

    pub fn behave(&self, change: impl FnOnce(&mut Behaviour)) {
        change(&mut self.lock().behaviour);
    }

    pub fn authorize_requests(&self) -> Vec<AuthorizeRequest> {
        self.lock().authorize_requests.clone()
    }

    pub fn token_requests(&self) -> Vec<TokenRequest> {
        self.lock().token_requests.clone()
    }

    pub fn discovery_hits(&self) -> usize {
        self.lock().discovery_hits
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.shared.inner.lock().expect("the mock state lock")
    }
}

fn fresh_key() -> SigningKey {
    loop {
        let bytes = random_bytes::<32>().expect("entropy is available");
        if let Ok(key) = SigningKey::from_slice(&bytes) {
            return key;
        }
    }
}

fn b64(bytes: impl AsRef<[u8]>) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn lock(shared: &Shared) -> MutexGuard<'_, Inner> {
    shared.inner.lock().expect("the mock state lock")
}

async fn discovery(State(shared): State<Arc<Shared>>) -> Response {
    lock(&shared).discovery_hits += 1;
    let base = &shared.base;
    axum::Json(json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/authorize"),
        "token_endpoint": format!("{base}/token"),
        "jwks_uri": format!("{base}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["ES256"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic"],
        "code_challenge_methods_supported": ["S256"],
    }))
    .into_response()
}

async fn jwks(State(shared): State<Arc<Shared>>) -> Response {
    let point = shared.key.verifying_key().to_sec1_point(false);
    let bytes = point.as_bytes();
    axum::Json(json!({
        "keys": [{
            "kty": "EC",
            "crv": "P-256",
            "alg": "ES256",
            "use": "sig",
            "kid": KEY_ID,
            "x": b64(&bytes[1..33]),
            "y": b64(&bytes[33..65]),
        }]
    }))
    .into_response()
}

fn query_map(raw: &str) -> HashMap<String, String> {
    form_urlencoded::parse(raw.as_bytes())
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

async fn authorize(State(shared): State<Arc<Shared>>, RawQuery(query): RawQuery) -> Response {
    let params = query_map(&query.unwrap_or_default());
    let field = |name: &str| params.get(name).cloned().unwrap_or_default();
    let request = AuthorizeRequest {
        client_id: field("client_id"),
        redirect_uri: field("redirect_uri"),
        scope: field("scope"),
        state: field("state"),
        nonce: field("nonce"),
        code_challenge: field("code_challenge"),
        code_challenge_method: field("code_challenge_method"),
    };
    if field("response_type") != "code" || request.client_id != CLIENT_ID {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let code = b64(random_bytes::<16>().expect("entropy is available"));
    let mut inner = lock(&shared);
    let state = if inner.behaviour.tamper_state {
        "A".repeat(43)
    } else {
        request.state.clone()
    };
    let mut target = Url::parse(&request.redirect_uri).expect("the redirect URI parses");
    target
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &state);
    inner.authorize_requests.push(request.clone());
    inner.issued.insert(code, Issued { request });
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(target.as_str()).expect("the location is a header value"),
    );
    response
}

fn oauth_error(code: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(json!({ "error": code })),
    )
        .into_response()
}

async fn token(State(shared): State<Arc<Shared>>, headers: HeaderMap, body: Bytes) -> Response {
    let form = query_map(&String::from_utf8_lossy(&body));
    let field = |name: &str| form.get(name).cloned().unwrap_or_default();
    let basic_credentials = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Basic "))
        .and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok())
        .and_then(|bytes| String::from_utf8(bytes).ok());
    let mut inner = lock(&shared);
    inner.token_requests.push(TokenRequest {
        code_verifier: field("code_verifier"),
        redirect_uri: field("redirect_uri"),
        basic_credentials: basic_credentials.clone(),
    });
    if inner.behaviour.reject_token_request {
        return oauth_error("invalid_grant");
    }
    if field("grant_type") != "authorization_code" {
        return oauth_error("unsupported_grant_type");
    }
    let expected = format!("{CLIENT_ID}:{CLIENT_SECRET}");
    if basic_credentials.as_deref() != Some(expected.as_str()) {
        return oauth_error("invalid_client");
    }
    let Some(issued) = inner.issued.remove(&field("code")) else {
        return oauth_error("invalid_grant");
    };
    let challenge = b64(Sha256::digest(field("code_verifier").as_bytes()));
    if issued.request.code_challenge_method != "S256"
        || challenge != issued.request.code_challenge
        || field("redirect_uri") != issued.request.redirect_uri
    {
        return oauth_error("invalid_grant");
    }
    let behaviour = inner.behaviour.clone();
    drop(inner);

    let mut payload = json!({
        "access_token": "mock-access-token",
        "token_type": "Bearer",
        "expires_in": 3600,
    });
    if !behaviour.omit_id_token {
        payload["id_token"] = Value::String(id_token(&shared, &behaviour, &issued.request));
    }
    axum::Json(payload).into_response()
}

fn id_token(shared: &Shared, behaviour: &Behaviour, request: &AuthorizeRequest) -> String {
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_secs(),
    )
    .expect("the time fits in i64");
    let mut claims = json!({
        "iss": behaviour.issuer_claim.clone().unwrap_or_else(|| shared.base.clone()),
        "sub": behaviour.subject,
        "aud": behaviour.audience_claim.clone().unwrap_or_else(|| CLIENT_ID.to_owned()),
        "exp": now + behaviour.expires_in_seconds,
        "iat": now - 5,
        "nonce": behaviour.nonce_claim.clone().unwrap_or_else(|| request.nonce.clone()),
    });
    if let Some(email) = &behaviour.email {
        claims["email"] = json!(email);
    }
    if let Some(verified) = behaviour.email_verified {
        claims["email_verified"] = json!(verified);
    }
    if let Some(name) = &behaviour.name {
        claims["name"] = json!(name);
    }
    let header = json!({ "alg": "ES256", "kid": KEY_ID, "typ": "JWT" });
    let signing_input = format!("{}.{}", b64(header.to_string()), b64(claims.to_string()));
    let key = if behaviour.bad_signature {
        &shared.foreign_key
    } else {
        &shared.key
    };
    let signature: p256::ecdsa::Signature = key.sign(signing_input.as_bytes());
    format!("{signing_input}.{}", b64(signature.to_bytes()))
}
