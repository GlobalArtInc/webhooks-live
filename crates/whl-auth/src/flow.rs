use axum::extract::{RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use url::form_urlencoded;
use whl_store::LoginRecord;

use crate::cookies::{self, CookieError};
use crate::error::{AuthRejection, LoginFailure};
use crate::return_to;
use crate::state::AuthState;
use crate::token::{RandomToken, login_key};

const MAX_CODE_LENGTH: usize = 4096;

struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    provider_error: bool,
}

impl CallbackQuery {
    fn parse(raw: Option<&str>) -> Self {
        let mut query = Self {
            code: None,
            state: None,
            provider_error: false,
        };
        for (name, value) in form_urlencoded::parse(raw.unwrap_or_default().as_bytes()) {
            match name.as_ref() {
                "code" if query.code.is_none() => query.code = Some(value.into_owned()),
                "state" if query.state.is_none() => query.state = Some(value.into_owned()),
                "error" => query.provider_error = true,
                _ => {}
            }
        }
        query
    }
}

fn query_value(raw: Option<&str>, key: &str) -> Option<String> {
    form_urlencoded::parse(raw.unwrap_or_default().as_bytes())
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

fn found(
    location: &str,
    cookie_values: impl IntoIterator<Item = Option<HeaderValue>>,
) -> Result<Response, FlowError> {
    let location = HeaderValue::from_str(location).map_err(|_| FlowError::Location)?;
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(header::LOCATION, location);
    cookies::append(response.headers_mut(), cookie_values)?;
    Ok(response)
}

#[derive(Debug, thiserror::Error)]
enum FlowError {
    #[error("the redirect target is not a valid header value")]
    Location,
    #[error(transparent)]
    Cookie(#[from] CookieError),
}

fn failed_login(auth: &AuthState, failure: LoginFailure) -> Response {
    tracing::warn!(code = failure.code(), "sign-in failed");
    let location = format!("/?login_error={}", failure.code());
    match found(&location, [auth.cookies().clear_login()]) {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(%error, "cannot build the sign-in failure redirect");
            AuthRejection::Internal.into_response()
        }
    }
}

pub(crate) async fn login(State(auth): State<AuthState>, RawQuery(query): RawQuery) -> Response {
    match start_login(&auth, query.as_deref()).await {
        Ok(response) => response,
        Err(failure) => failed_login(&auth, failure),
    }
}

async fn start_login(auth: &AuthState, query: Option<&str>) -> Result<Response, LoginFailure> {
    let destination = return_to::sanitize(query_value(query, "return_to").as_deref());
    let state = random()?;
    let nonce = random()?;
    let binding = random()?;
    let request = auth
        .provider()
        .authorization_request(state.unpack(), nonce.unpack())
        .await;
    let record = LoginRecord {
        pkce_verifier: request.pkce_verifier,
        nonce: nonce.unpack().to_owned(),
        return_to: destination,
    };
    auth.store()
        .create_login_state(
            &login_key(state.unpack(), &binding),
            &record,
            auth.settings().login_ttl(),
        )
        .await?;
    let cookie = auth
        .cookies()
        .set_login(&binding, auth.settings().login_ttl());
    found(request.url.as_str(), [cookie]).map_err(|error| {
        tracing::error!(%error, "cannot build the authorization redirect");
        LoginFailure::Internal
    })
}

pub(crate) async fn callback(
    State(auth): State<AuthState>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    match finish_login(&auth, &headers, query.as_deref()).await {
        Ok(response) => response,
        Err(failure) => failed_login(&auth, failure),
    }
}

async fn finish_login(
    auth: &AuthState,
    headers: &HeaderMap,
    raw_query: Option<&str>,
) -> Result<Response, LoginFailure> {
    let query = CallbackQuery::parse(raw_query);
    let binding = auth
        .cookies()
        .read_login(headers)
        .ok_or(LoginFailure::InvalidState)?;
    let state = query
        .state
        .as_deref()
        .and_then(RandomToken::parse)
        .ok_or(LoginFailure::InvalidState)?;
    let record = auth
        .store()
        .take_login_state(&login_key(state.unpack(), &binding))
        .await?
        .ok_or(LoginFailure::InvalidState)?;
    if query.provider_error {
        return Err(LoginFailure::ProviderDenied);
    }
    let code = query
        .code
        .filter(|code| !code.is_empty() && code.len() <= MAX_CODE_LENGTH)
        .ok_or(LoginFailure::InvalidRequest)?;
    let identity = auth
        .provider()
        .complete(&code, &record.pkce_verifier, &record.nonce)
        .await?;
    let user = auth.store().upsert_user(&identity).await?;
    let token = random()?;
    auth.store()
        .create_session(
            token.digest().unpack(),
            user.id,
            auth.settings().limits().max,
        )
        .await?;
    if let Some(previous) = auth.session_hash(headers)
        && let Err(error) = auth.store().delete_session(previous.unpack()).await
    {
        tracing::warn!(%error, "cannot delete the session the browser presented before sign-in");
    }
    tracing::info!(user_id = %user.id, "user signed in");
    let destination = return_to::sanitize(Some(&record.return_to));
    found(
        &destination,
        [
            auth.cookies().set_session(&token),
            auth.cookies().clear_login(),
        ],
    )
    .map_err(|error| {
        tracing::error!(%error, "cannot build the sign-in redirect");
        LoginFailure::Internal
    })
}

pub(crate) async fn logout(State(auth): State<AuthState>, headers: HeaderMap) -> Response {
    if let Some(hash) = auth.session_hash(&headers)
        && let Err(error) = auth.store().delete_session(hash.unpack()).await
    {
        return AuthRejection::from(error).into_response();
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    let cleared = cookies::append(response.headers_mut(), [auth.cookies().clear_session()]);
    match cleared {
        Ok(()) => response,
        Err(error) => {
            tracing::error!(%error, "cannot build the session-clearing cookie");
            AuthRejection::Internal.into_response()
        }
    }
}

fn random() -> Result<RandomToken, LoginFailure> {
    RandomToken::generate().map_err(|error| {
        tracing::error!(%error, "the operating system random source failed");
        LoginFailure::Internal
    })
}
