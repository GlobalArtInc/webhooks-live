use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreErrorResponseType, CoreIdToken, CoreIdTokenClaims,
    CoreJwsSigningAlgorithm, CoreProviderMetadata,
};
use openidconnect::{
    AuthorizationCode, ClaimsVerificationError, CsrfToken, EndpointMaybeSet, EndpointNotSet,
    EndpointSet, IssuerUrl, Nonce, PkceCodeChallenge, PkceCodeVerifier, RequestTokenError,
    SignatureVerificationError, StandardErrorResponse, TokenResponse, reqwest,
};
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use url::Url;
use whl_store::VerifiedIdentity;

use crate::error::{AuthError, LoginFailure};
use crate::settings::AuthSettings;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const METADATA_TTL: Duration = Duration::from_secs(3600);
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);
const MIN_FORCED_INTERVAL: Duration = Duration::from_secs(60);
pub const CLOCK_LEEWAY: Duration = Duration::from_secs(60);

const ALLOWED_ALGORITHMS: [CoreJwsSigningAlgorithm; 8] = [
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512,
    CoreJwsSigningAlgorithm::RsaSsaPssSha256,
    CoreJwsSigningAlgorithm::RsaSsaPssSha384,
    CoreJwsSigningAlgorithm::RsaSsaPssSha512,
    CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    CoreJwsSigningAlgorithm::EcdsaP384Sha384,
];

type ConfiguredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

struct Snapshot {
    metadata: Arc<CoreProviderMetadata>,
    refresh_at: Instant,
    attempted_at: Instant,
}

pub struct AuthorizationRequest {
    pub url: Url,
    pub pkce_verifier: String,
}

pub struct Provider {
    http: reqwest::Client,
    settings: AuthSettings,
    issuer: IssuerUrl,
    snapshot: RwLock<Snapshot>,
    refresh_gate: Mutex<()>,
}

pub(crate) fn http_client() -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|error| AuthError::HttpClient(error.to_string()))
}

impl Provider {
    pub async fn discover(
        http: reqwest::Client,
        settings: AuthSettings,
    ) -> Result<Self, AuthError> {
        let (issuer, metadata) = fetch_any(&http, &settings).await?;
        let now = Instant::now();
        tracing::info!(issuer = issuer.as_str(), "OIDC discovery succeeded");
        Ok(Self {
            http,
            settings,
            issuer,
            snapshot: RwLock::new(Snapshot {
                metadata: Arc::new(metadata),
                refresh_at: now + METADATA_TTL,
                attempted_at: now,
            }),
            refresh_gate: Mutex::new(()),
        })
    }

    pub async fn authorization_request(&self, state: &str, nonce: &str) -> AuthorizationRequest {
        let metadata = self.current().await;
        let client = self.client(&metadata);
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let state = state.to_owned();
        let nonce = nonce.to_owned();
        let mut request = client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                move || CsrfToken::new(state),
                move || Nonce::new(nonce),
            )
            .set_pkce_challenge(challenge);
        for scope in &self.settings.scopes {
            request = request.add_scope(scope.clone());
        }
        let (url, _, _) = request.url();
        AuthorizationRequest {
            url,
            pkce_verifier: verifier.secret().clone(),
        }
    }

    pub async fn complete(
        &self,
        code: &str,
        pkce_verifier: &str,
        nonce: &str,
    ) -> Result<VerifiedIdentity, LoginFailure> {
        let metadata = self.current().await;
        let client = self.client(&metadata);
        let token_request = client.exchange_code(AuthorizationCode::new(code.to_owned()));
        let token_request = match token_request {
            Ok(request) => request,
            Err(error) => {
                tracing::error!(%error, "the provider metadata has no token endpoint");
                return Err(LoginFailure::ExchangeFailed);
            }
        };
        let response = token_request
            .set_pkce_verifier(PkceCodeVerifier::new(pkce_verifier.to_owned()))
            .request_async(&self.http)
            .await
            .map_err(|error| {
                tracing::warn!(kind = request_error_kind(&error), "token exchange failed");
                match &error {
                    RequestTokenError::Parse(_, body) => classify_unparseable(body),
                    _ => LoginFailure::ExchangeFailed,
                }
            })?;
        let Some(id_token) = response.id_token() else {
            tracing::warn!("the token response carries no ID token");
            return Err(LoginFailure::InvalidToken);
        };
        let nonce = Nonce::new(nonce.to_owned());
        let claims = match self.verify(&client, id_token, &nonce) {
            Err(ClaimsVerificationError::SignatureVerification(
                SignatureVerificationError::NoMatchingKey,
            )) => {
                let refreshed = self.force_refresh().await;
                let client = self.client(&refreshed);
                self.verify(&client, id_token, &nonce)
            }
            outcome => outcome,
        }
        .map_err(|error| {
            tracing::warn!(%error, "the ID token did not verify");
            LoginFailure::InvalidToken
        })?;
        let payload = serde_json::to_value(id_token).ok().and_then(|token| {
            token
                .as_str()
                .and_then(|compact| payload_of(compact.as_bytes()))
        });
        let not_before = match payload.as_ref().map(not_before_of) {
            Some(NotBefore::Absent) => None,
            Some(NotBefore::At(seconds)) => Some(seconds),
            Some(NotBefore::Malformed) | None => {
                tracing::warn!("the ID token payload cannot be inspected");
                return Err(LoginFailure::InvalidToken);
            }
        };
        let audiences: Vec<&str> = claims
            .audiences()
            .iter()
            .map(|audience| audience.as_str())
            .collect();
        let authorized_party = claims.authorized_party().map(|party| party.as_str());
        if !audience_is_valid(
            &audiences,
            authorized_party,
            self.settings.client_id.as_str(),
        ) {
            tracing::warn!("the ID token has an authorized party that is not this client");
            return Err(LoginFailure::InvalidToken);
        }
        if !times_are_valid(
            claims.issue_time().timestamp(),
            not_before,
            unix_now(),
            leeway_seconds(),
        ) {
            tracing::warn!("the ID token was issued or becomes valid in the future");
            return Err(LoginFailure::InvalidToken);
        }
        identity_from(claims)
    }

    fn verify(
        &self,
        client: &ConfiguredClient,
        id_token: &CoreIdToken,
        nonce: &Nonce,
    ) -> Result<CoreIdTokenClaims, ClaimsVerificationError> {
        let verifier = client
            .id_token_verifier()
            .set_allowed_algs(ALLOWED_ALGORITHMS);
        id_token.claims(&verifier, nonce).cloned()
    }

    async fn current(&self) -> Arc<CoreProviderMetadata> {
        {
            let snapshot = self.snapshot.read().await;
            if Instant::now() < snapshot.refresh_at {
                return Arc::clone(&snapshot.metadata);
            }
        }
        self.refresh(Refresh::Expired).await
    }

    async fn force_refresh(&self) -> Arc<CoreProviderMetadata> {
        self.refresh(Refresh::UnknownKey).await
    }

    async fn refresh(&self, reason: Refresh) -> Arc<CoreProviderMetadata> {
        let _gate = self.refresh_gate.lock().await;
        {
            let snapshot = self.snapshot.read().await;
            let now = Instant::now();
            let due = match reason {
                Refresh::Expired => now >= snapshot.refresh_at,
                Refresh::UnknownKey => {
                    now.duration_since(snapshot.attempted_at) >= MIN_FORCED_INTERVAL
                }
            };
            if !due {
                return Arc::clone(&snapshot.metadata);
            }
        }
        let outcome = fetch(&self.http, &self.issuer).await;
        let now = Instant::now();
        let mut snapshot = self.snapshot.write().await;
        snapshot.attempted_at = now;
        match outcome {
            Ok(metadata) => {
                tracing::info!(issuer = self.issuer.as_str(), "OIDC metadata refreshed");
                snapshot.metadata = Arc::new(metadata);
                snapshot.refresh_at = now + METADATA_TTL;
            }
            Err(error) => {
                tracing::warn!(%error, "OIDC metadata refresh failed, keeping the cached copy");
                snapshot.refresh_at = now + RETRY_AFTER_FAILURE;
            }
        }
        Arc::clone(&snapshot.metadata)
    }

    fn client(&self, metadata: &CoreProviderMetadata) -> ConfiguredClient {
        CoreClient::from_provider_metadata(
            metadata.clone(),
            self.settings.client_id.clone(),
            Some(self.settings.client_secret.clone()),
        )
        .set_redirect_uri(self.settings.redirect_url.clone())
    }
}

#[derive(Debug, Clone, Copy)]
enum Refresh {
    Expired,
    UnknownKey,
}

async fn fetch_any(
    http: &reqwest::Client,
    settings: &AuthSettings,
) -> Result<(IssuerUrl, CoreProviderMetadata), AuthError> {
    let mut first_error = None;
    for candidate in issuer_candidates(&settings.issuer) {
        match fetch(http, &candidate).await {
            Ok(metadata) => return Ok((candidate, metadata)),
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    Err(first_error.unwrap_or_else(|| AuthError::Discovery {
        issuer: settings.issuer.as_str().to_owned(),
        reason: "no issuer candidate".to_owned(),
    }))
}

fn issuer_candidates(configured: &IssuerUrl) -> Vec<IssuerUrl> {
    let text = configured.as_str();
    let bare = text.strip_suffix('/').unwrap_or(text);
    let has_path = bare
        .split_once("://")
        .is_some_and(|(_, rest)| rest.contains('/'));
    if has_path {
        return vec![configured.clone()];
    }
    [bare.to_owned(), format!("{bare}/")]
        .into_iter()
        .filter_map(|candidate| IssuerUrl::new(candidate).ok())
        .collect()
}

async fn fetch(
    http: &reqwest::Client,
    issuer: &IssuerUrl,
) -> Result<CoreProviderMetadata, AuthError> {
    CoreProviderMetadata::discover_async(issuer.clone(), http)
        .await
        .map_err(|error| AuthError::Discovery {
            issuer: issuer.as_str().to_owned(),
            reason: error.to_string(),
        })
}

fn request_error_kind<E: std::error::Error>(
    error: &RequestTokenError<E, StandardErrorResponse<CoreErrorResponseType>>,
) -> &'static str {
    match error {
        RequestTokenError::ServerResponse(_) => "server_response",
        RequestTokenError::Request(_) => "request",
        RequestTokenError::Parse(_, _) => "parse",
        RequestTokenError::Other(_) => "other",
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(i64::MAX)
}

fn leeway_seconds() -> i64 {
    i64::try_from(CLOCK_LEEWAY.as_secs()).unwrap_or(i64::MAX)
}

fn times_are_valid(issued_at: i64, not_before: Option<i64>, now: i64, leeway: i64) -> bool {
    let latest = now.saturating_add(leeway);
    issued_at <= latest && not_before.is_none_or(|not_before| not_before <= latest)
}

fn audience_is_valid(audiences: &[&str], authorized_party: Option<&str>, client_id: &str) -> bool {
    match authorized_party {
        Some(party) => party == client_id,
        None => audiences.len() <= 1,
    }
}

fn payload_of(compact: &[u8]) -> Option<serde_json::Value> {
    let mut segments = compact.split(|byte| *byte == b'.');
    let payload = segments.nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&decoded).ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotBefore {
    Absent,
    At(i64),
    Malformed,
}

fn not_before_of(payload: &serde_json::Value) -> NotBefore {
    match payload.get("nbf") {
        None | Some(serde_json::Value::Null) => NotBefore::Absent,
        Some(value) => value.as_i64().map_or(NotBefore::Malformed, NotBefore::At),
    }
}

fn classify_unparseable(body: &[u8]) -> LoginFailure {
    let id_token = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|response| response.get("id_token").cloned());
    let payload = id_token
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .and_then(|compact| payload_of(compact.as_bytes()));
    match payload
        .as_ref()
        .and_then(|claims| claims.get("email_verified"))
    {
        Some(serde_json::Value::Bool(_) | serde_json::Value::Null) | None => {
            LoginFailure::ExchangeFailed
        }
        Some(_) => LoginFailure::EmailVerifiedInvalid,
    }
}

fn identity_from(claims: CoreIdTokenClaims) -> Result<VerifiedIdentity, LoginFailure> {
    let Some(email) = claims.email().map(|email| email.as_str().to_owned()) else {
        return Err(LoginFailure::MissingEmail);
    };
    if email.is_empty() {
        return Err(LoginFailure::MissingEmail);
    }
    if claims.email_verified() == Some(false) {
        return Err(LoginFailure::EmailNotVerified);
    }
    let display_name = claims
        .name()
        .and_then(|name| name.get(None))
        .map(|name| name.as_str().to_owned())
        .filter(|name| !name.is_empty());
    Ok(VerifiedIdentity {
        issuer: claims.issuer().as_str().to_owned(),
        subject: claims.subject().as_str().to_owned(),
        email,
        email_verified: claims.email_verified() == Some(true),
        display_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(text: &str) -> Vec<String> {
        let issuer = IssuerUrl::new(text.to_owned()).expect("a valid issuer");
        issuer_candidates(&issuer)
            .iter()
            .map(|candidate| candidate.as_str().to_owned())
            .collect()
    }

    #[test]
    fn a_bare_host_issuer_is_tried_without_and_then_with_the_trailing_slash() {
        assert_eq!(
            candidates("https://sso.example/"),
            ["https://sso.example", "https://sso.example/"]
        );
        assert_eq!(
            candidates("https://sso.example"),
            ["https://sso.example", "https://sso.example/"]
        );
    }

    #[test]
    fn an_issuer_with_a_path_is_used_exactly_as_configured() {
        assert_eq!(
            candidates("https://sso.example/realms/main"),
            ["https://sso.example/realms/main"]
        );
        assert_eq!(
            candidates("https://sso.example/realms/main/"),
            ["https://sso.example/realms/main/"]
        );
    }

    #[test]
    fn issue_and_not_before_times_may_lead_the_clock_only_by_the_leeway() {
        let now = 1_000_000;
        let leeway = leeway_seconds();
        assert!(times_are_valid(now, None, now, leeway));
        assert!(times_are_valid(now + leeway, None, now, leeway));
        assert!(!times_are_valid(now + leeway + 1, None, now, leeway));
        assert!(times_are_valid(now - 5, Some(now + leeway), now, leeway));
        assert!(!times_are_valid(
            now - 5,
            Some(now + leeway + 1),
            now,
            leeway
        ));
        assert!(times_are_valid(now - 5, Some(now - 3600), now, leeway));
    }

    #[test]
    fn a_multi_valued_audience_needs_this_client_as_authorized_party() {
        assert!(audience_is_valid(&["me"], None, "me"));
        assert!(audience_is_valid(&["me"], Some("me"), "me"));
        assert!(!audience_is_valid(&["me"], Some("other"), "me"));
        assert!(!audience_is_valid(&["me", "other"], None, "me"));
        assert!(!audience_is_valid(&["me", "other"], Some("other"), "me"));
        assert!(audience_is_valid(&["me", "other"], Some("me"), "me"));
    }

    #[test]
    fn not_before_is_read_as_a_whole_number_or_flagged() {
        use serde_json::json;
        assert_eq!(not_before_of(&json!({})), NotBefore::Absent);
        assert_eq!(not_before_of(&json!({ "nbf": null })), NotBefore::Absent);
        assert_eq!(not_before_of(&json!({ "nbf": 12 })), NotBefore::At(12));
        assert_eq!(not_before_of(&json!({ "nbf": "12" })), NotBefore::Malformed);
        assert_eq!(not_before_of(&json!({ "nbf": 12.5 })), NotBefore::Malformed);
    }

    fn response_with(claims: &serde_json::Value) -> Vec<u8> {
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        serde_json::json!({ "id_token": format!("e30.{payload}.c2ln") })
            .to_string()
            .into_bytes()
    }

    #[test]
    fn an_unparseable_token_response_names_a_non_boolean_email_verified() {
        use serde_json::json;
        for claim in [json!("true"), json!("false"), json!(1), json!([true])] {
            let body = response_with(&json!({ "email_verified": claim }));
            assert_eq!(
                classify_unparseable(&body),
                LoginFailure::EmailVerifiedInvalid,
                "{claim}"
            );
        }
        for body in [
            response_with(&json!({ "email_verified": true })),
            response_with(&json!({ "email_verified": null })),
            response_with(&json!({})),
            b"not json".to_vec(),
            b"{}".to_vec(),
        ] {
            assert_eq!(classify_unparseable(&body), LoginFailure::ExchangeFailed);
        }
    }
}
