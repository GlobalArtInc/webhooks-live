use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use whl_auth::{AuthRejection, Caller};
use whl_notify::PushService;
use whl_store::Uuid;
use whl_store::notify::{NewPushSubscription, SavePushOutcome};

use super::dto::{
    CreatedDto, PublicKeyDto, PushSubscriptionInputDoc, SubscriptionDto, SubscriptionListDto,
};
use super::input::parse_subscription;
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody};
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, Unauthenticated, UnsupportedMediaType,
};
use crate::state::ApiState;

#[utoipa::path(
    get,
    path = "/push/public-key",
    operation_id = "push_public_key",
    tag = "Push",
    summary = "The VAPID public key of web push",
    description = "Public. The key a browser passes as `applicationServerKey` when it subscribes to push. Answers 503 `feature_disabled` when web push is not configured.",
    security(()),
    responses(
        (status = 200, description = "The public key, base64url", body = PublicKeyDto),
        (status = 503, description = "`feature_disabled`: web push is not configured on this instance", body = ErrorBody)
    )
)]
pub async fn public_key(State(state): State<ApiState>) -> Result<Json<PublicKeyDto>, ApiError> {
    let push = push_service(&state)?;
    Ok(Json(PublicKeyDto {
        public_key: push.public_key().to_owned(),
    }))
}

#[utoipa::path(
    get,
    path = "/push/subscriptions",
    operation_id = "push_list",
    tag = "Push",
    summary = "List the caller's push subscriptions",
    description = "Signed in only: the caller's own browser subscriptions. The endpoint URL and keys are never returned. Answers 503 `feature_disabled` when web push is not configured.",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "The subscriptions", body = SubscriptionListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`sign_in_required`: no signed-in caller on a `mixed` instance; `not_available_in_mode`: the instance has no accounts; `session_required`: a Bearer token was used", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: web push is not configured; `store_unavailable`", body = ErrorBody)
    )
)]
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

#[utoipa::path(
    post,
    path = "/push/subscriptions",
    operation_id = "push_create",
    tag = "Push",
    summary = "Register a browser push subscription",
    description = "Signed in only. The body is the browser's `PushSubscription` JSON. `endpoint` must be an HTTPS URL on a push service host the instance accepts (by default FCM, Mozilla autopush, Apple and WNS); `keys` are base64url. The `User-Agent` header is stored with it. At most 10 subscriptions per account; the same endpoint twice answers 422 `already_registered`. Answers 503 `feature_disabled` when web push is not configured.",
    security(("session_cookie" = [])),
    request_body(content = PushSubscriptionInputDoc, content_type = "application/json"),
    responses(
        (status = 201, description = "The subscription is saved", body = CreatedDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`sign_in_required`: no signed-in caller on a `mixed` instance; `not_available_in_mode`: the instance has no accounts; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (`error.fields`): `host_not_allowed` for an endpoint outside the accepted push hosts; `invalid_format` or `too_long` for `endpoint` or the keys; `not_allowed` for an unknown field; `limit_reached` (`error.limit` = `{name: \"push_subscriptions\", max: 10}`); `already_registered`", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: web push is not configured; `store_unavailable`", body = ErrorBody)
    )
)]
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

#[utoipa::path(
    delete,
    path = "/push/subscriptions/{subscription_id}",
    operation_id = "push_delete",
    tag = "Push",
    summary = "Remove a push subscription",
    description = "Signed in only. Only the caller's own subscriptions; another account's id answers 404 `subscription_not_found`.",
    security(("session_cookie" = [])),
    params(("subscription_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Subscription id, lowercase hyphenated UUID")),
    responses(
        (status = 204, description = "The subscription is removed"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`sign_in_required`: no signed-in caller on a `mixed` instance; `not_available_in_mode`: the instance has no accounts; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`subscription_not_found`: unknown, malformed or another account's id", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: web push is not configured; `store_unavailable`", body = ErrorBody)
    )
)]
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
