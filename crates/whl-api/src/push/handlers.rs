use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use whl_auth::{AuthRejection, Caller};
use whl_notify::PushService;
use whl_store::Uuid;
use whl_store::notify::{NewPushSubscription, SavePushOutcome};

use super::dto::{CreatedDto, PublicKeyDto, SubscriptionDto, SubscriptionListDto};
use super::input::parse_subscription;
use crate::body::RawBody;
use crate::error::ApiError;
use crate::state::ApiState;

pub async fn public_key(State(state): State<ApiState>) -> Result<Json<PublicKeyDto>, ApiError> {
    let push = push_service(&state)?;
    Ok(Json(PublicKeyDto {
        public_key: push.public_key().to_owned(),
    }))
}

pub async fn list(
    State(state): State<ApiState>,
    caller: Caller,
) -> Result<Json<SubscriptionListDto>, ApiError> {
    push_service(&state)?;
    let user = signed_in(&state, &caller)?;
    let items = state.store.list_push_subscriptions(user).await?;
    Ok(Json(SubscriptionListDto {
        items: items.iter().map(SubscriptionDto::from).collect(),
    }))
}

pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    headers: HeaderMap,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let push = push_service(&state)?;
    let user = signed_in(&state, &caller)?;
    let parsed = parse_subscription(&body, push.hosts())?;
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let subscription = NewPushSubscription {
        id: Uuid::now_v7(),
        user_id: user,
        endpoint_url: parsed.endpoint,
        host: parsed.host,
        p256dh: parsed.p256dh,
        auth: parsed.auth,
        user_agent,
    };
    match state.store.save_push_subscription(&subscription).await? {
        SavePushOutcome::Saved(saved) => {
            tracing::info!(subscription = %saved.id, "push subscription saved");
            Ok((StatusCode::CREATED, Json(CreatedDto::from(&saved))).into_response())
        }
        SavePushOutcome::LimitReached => Err(ApiError::SubscriptionLimit),
        SavePushOutcome::AlreadyRegistered => Err(ApiError::AlreadyRegistered),
        SavePushOutcome::UserGone => Err(ApiError::Auth(AuthRejection::Unauthenticated)),
    }
}

pub async fn delete(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    push_service(&state)?;
    let user = signed_in(&state, &caller)?;
    let id = subscription_id(&raw_id)?;
    if state.store.delete_push_subscription(user, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::SubscriptionNotFound)
    }
}

fn push_service(state: &ApiState) -> Result<&PushService, ApiError> {
    state.push.as_ref().ok_or(ApiError::PushDisabled)
}

fn signed_in(state: &ApiState, caller: &Caller) -> Result<Uuid, ApiError> {
    if !state.access.has_users() {
        return Err(ApiError::NotAvailableInMode);
    }
    caller
        .user()
        .map(|user| user.id)
        .ok_or(ApiError::Auth(AuthRejection::SignInRequired))
}

fn subscription_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw)
        .ok_or(ApiError::SubscriptionNotFound)
}
