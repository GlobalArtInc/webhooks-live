use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use whl_auth::{AuthRejection, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_core::tier::Tier;
use whl_forward::vet_target;
use whl_notify::{channel_secret_name, mask_url, targets_instance};
use whl_secrets::{Keyring, Secret};
use whl_store::Uuid;
use whl_store::notify::{
    ChannelKind, ChannelPatch, ConfigChange, CreateChannelOutcome, DeleteChannelOutcome,
    NewChannel, TestOutcome, Trigger, UpdateChannelOutcome,
};

use super::dto::{ChannelDto, ChannelInputDoc, ChannelListDto, DeliveryDto};
use super::email::signed_in_user;
use super::input::{ChannelConfig, DEFAULT_PER_HOUR, ParsedChannel, parse_channel};
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::forward::target_violation_for;
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, RateLimited, StoreUnavailable, Unauthenticated,
    UnsupportedMediaType,
};
use crate::state::ApiState;

const TEST_RETRY_AFTER_SECS: u64 = 60;

#[utoipa::path(
    get,
    path = "/endpoints/{id}/notifications",
    operation_id = "notifications_list",
    tag = "Notifications",
    summary = "List the notification channels of an endpoint",
    description = "Viewer role on the endpoint. Returns every channel, enabled or not. `config` never holds a full URL: see the Channel schema. A Bearer token is refused with 403 `session_required`. Answers 404 `not_found` when notifications are switched off on this instance.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    responses(
        (status = 200, description = "The channels", body = ChannelListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: notifications are switched off, or the endpoint id is malformed", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<ChannelListDto>, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let channels = state.store.list_channels(&endpoint).await?;
    Ok(Json(ChannelListDto {
        items: channels.iter().map(ChannelDto::from).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/notifications",
    operation_id = "notifications_create",
    tag = "Notifications",
    summary = "Create a notification channel",
    description = "Editor role on the endpoint; at most 10 channels per endpoint. `kind`, `name` and `config` are required. Defaults: `enabled` true, `trigger` `capture`, `template` null, `per_hour` 30. `condition` must be null. `email` and `push` need a signed-in caller; `push` also needs a `push` instance with accounts. Webhook, Slack and Discord URLs are sealed with the instance keyring and only their masked form is returned. Without the keyring the route answers 503 `feature_disabled`, checked before the body is validated. A `kind` the tier does not allow answers 403 `tier_not_allowed` with `error.limit.name` = the kind. Bearer tokens are refused with 403 `session_required`.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    request_body(content = ChannelInputDoc, content_type = "application/json"),
    responses(
        (status = 201, description = "The created channel", body = ChannelDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `forbidden`: viewer role; `csrf_rejected`: the Origin check failed; `tier_not_allowed`: the kind is not in the tier; `sign_in_required`: `push` or `email` without a signed-in caller; `not_available_in_mode`: `push` on an instance without accounts", body = ErrorBody),
        (status = 404, description = "`not_found`: notifications are switched off, or the endpoint id is malformed", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (`error.fields`): `not_allowed` for an unknown key, a non-null `condition`, a `kind` not yet available, or an address that is not a verified address of the caller or an e-mail channel on an endpoint without an owner; `invalid_format` for a Slack or Discord URL outside its pattern, a webhook URL that is masked or malformed, an unknown secret name, or a bad `chat_id`; `target_refused` for a webhook URL that is refused by the forward target rules or points at this instance; `limit_reached` (`error.limit` = `{name: \"notification_channels\", max: 10}`)", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: no secrets keyring, or push is not configured; `store_unavailable`", body = ErrorBody)
    )
)]
pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    keyring(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let tier = state.endpoint_tier(&endpoint).await?;
    let channel = build_channel(&state, &caller, &endpoint, Uuid::now_v7(), tier, &body).await?;
    let kind = channel.kind;
    match state
        .store
        .create_channel(&endpoint, state.settings.retention, &channel)
        .await?
    {
        CreateChannelOutcome::Created(created) => {
            tracing::info!(endpoint = %endpoint, channel = %created.id, kind = kind.as_str(), "notification channel created");
            Ok((StatusCode::CREATED, Json(ChannelDto::from(&created))).into_response())
        }
        CreateChannelOutcome::EndpointGone => Err(ApiError::EndpointGone),
        CreateChannelOutcome::LimitReached => Err(ApiError::ChannelLimit),
        CreateChannelOutcome::SecretsMissing(_) => {
            Err(ApiError::Validation(vec![token_secret_violation()]))
        }
    }
}

pub(crate) async fn build_channel(
    state: &ApiState,
    caller: &Caller,
    endpoint: &EndpointId,
    id: Uuid,
    tier: Tier<'_>,
    body: &[u8],
) -> Result<NewChannel, ApiError> {
    let keyring = keyring(state)?;
    let mut parsed = parse_channel(body, true, None)?;
    match parsed.kind {
        Some(ChannelKind::Push) => ensure_push_ready(state, caller)?,
        Some(ChannelKind::Email) => {
            signed_in_user(state, caller)?;
        }
        _ => {}
    }
    if let Some(kind) = parsed.kind
        && !tier.limits().allows_notification_kind(kind.as_str())
    {
        return Err(ApiError::TierNotAllowed {
            name: kind.as_str(),
        });
    }
    let stored = resolve_config(state, keyring, endpoint, id, caller, &mut parsed).await?;
    let (Some(kind), Some(name), Some(change)) = (
        parsed.kind.filter(|_| parsed.violations.is_empty()),
        parsed.name.take(),
        stored,
    ) else {
        return Err(ApiError::Validation(parsed.violations));
    };
    Ok(NewChannel {
        id,
        kind,
        name,
        enabled: parsed.enabled.unwrap_or(true),
        trigger: parsed.trigger.unwrap_or(Trigger::Capture),
        config: change.config,
        sealed: change.sealed,
        template: parsed.template.take().flatten(),
        per_hour: parsed.per_hour.unwrap_or(DEFAULT_PER_HOUR),
        created_by: caller.user().map(|user| user.id),
    })
}

#[utoipa::path(
    patch,
    path = "/endpoints/{id}/notifications/{channel_id}",
    operation_id = "notifications_update",
    tag = "Notifications",
    summary = "Change a notification channel",
    description = "Editor role on the endpoint. Partial: absent fields are unchanged and `null` resets `template`. `kind` cannot change (422 `not_allowed`). A new `config` is checked like on create. An `email` channel: a request that only sets `enabled` to false works for any editor; any other change is refused with 403 `creator_only` unless the caller created the channel. Re-enabling an `email` channel without a new `config` re-checks its address. An empty body is 400 `invalid_json`. Bearer tokens are refused with 403 `session_required`.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("channel_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Channel id, lowercase hyphenated UUID")
    ),
    request_body(content = ChannelInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The changed channel", body = ChannelDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `forbidden`: viewer role; `csrf_rejected`: the Origin check failed; `creator_only`: an e-mail channel changed by someone other than its creator; `sign_in_required`: a re-enabled e-mail channel without a signed-in caller", body = ErrorBody),
        (status = 404, description = "`channel_not_found`: unknown channel of this endpoint; `not_found`: notifications are switched off, or an id is malformed", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (`error.fields`), with the same field codes as create", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: no secrets keyring, needed when `config` is sent; `store_unavailable`", body = ErrorBody)
    )
)]
pub async fn update(
    State(state): State<ApiState>,
    caller: Caller,
    Path((raw_id, raw_channel)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Json<ChannelDto>, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let channel_id = channel_id(&raw_channel)?;
    live_endpoint(&state, &endpoint).await?;
    let current = state
        .store
        .get_channel(&endpoint, channel_id)
        .await?
        .ok_or(ApiError::ChannelNotFound)?;
    let mut parsed = parse_channel(&body, false, Some(current.kind))?;
    let only_disables = parsed.enabled == Some(false)
        && parsed.name.is_none()
        && parsed.trigger.is_none()
        && parsed.template.is_none()
        && parsed.per_hour.is_none()
        && parsed.config.is_none();
    let creator_only = current.kind == ChannelKind::Email && !only_disables;
    let reenables_email = current.kind == ChannelKind::Email
        && (parsed.enabled == Some(true) || parsed.config.is_some());
    if creator_only {
        let creator = caller.user().map(|user| user.id);
        if creator.is_none() || creator != current.created_by {
            return Err(ApiError::ChannelCreatorOnly);
        }
    }
    let change = if parsed.config.is_some() {
        let keyring = keyring(&state)?;
        resolve_config(&state, keyring, &endpoint, channel_id, &caller, &mut parsed).await?
    } else {
        None
    };
    if reenables_email && change.is_none() && parsed.violations.is_empty() {
        let address = current
            .config
            .get("address_id")
            .and_then(|value| value.as_str())
            .and_then(|text| Uuid::try_parse(text).ok());
        let user = signed_in_user(&state, &caller)?;
        let mut violations = match address {
            Some(address) => email_violations(&state, &endpoint, user, address).await?,
            None => Vec::new(),
        };
        if address.is_none() {
            violations.push(FieldViolation::new(
                "config.address_id",
                FieldCode::NotAllowed,
                "the channel names no email address",
            ));
        }
        parsed.violations.append(&mut violations);
    }
    if !parsed.violations.is_empty() {
        return Err(ApiError::Validation(parsed.violations));
    }
    let patch = ChannelPatch {
        name: parsed.name.take(),
        enabled: parsed.enabled,
        trigger: parsed.trigger,
        config: change,
        template: parsed.template.take(),
        per_hour: parsed.per_hour,
    };
    match state
        .store
        .update_channel(&endpoint, state.settings.retention, channel_id, &patch)
        .await?
    {
        UpdateChannelOutcome::Updated(updated) => Ok(Json(ChannelDto::from(&updated))),
        UpdateChannelOutcome::EndpointGone => Err(ApiError::EndpointGone),
        UpdateChannelOutcome::ChannelNotFound => Err(ApiError::ChannelNotFound),
        UpdateChannelOutcome::SecretsMissing(_) => {
            Err(ApiError::Validation(vec![token_secret_violation()]))
        }
    }
}

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/notifications/{channel_id}",
    operation_id = "notifications_delete",
    tag = "Notifications",
    summary = "Delete a notification channel",
    description = "Editor role on the endpoint. Bearer tokens are refused with 403 `session_required`.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("channel_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Channel id, lowercase hyphenated UUID")
    ),
    responses(
        (status = 204, description = "The channel is deleted"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `forbidden`: viewer role; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`channel_not_found`: unknown channel of this endpoint; `not_found`: notifications are switched off, or an id is malformed", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_channel)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let channel_id = channel_id(&raw_channel)?;
    match state
        .store
        .delete_channel(&endpoint, state.settings.retention, channel_id)
        .await?
    {
        DeleteChannelOutcome::Deleted => Ok(StatusCode::NO_CONTENT),
        DeleteChannelOutcome::EndpointGone => Err(ApiError::EndpointGone),
        DeleteChannelOutcome::ChannelNotFound => Err(ApiError::ChannelNotFound),
    }
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/notifications/{channel_id}/test",
    operation_id = "notifications_test",
    tag = "Notifications",
    summary = "Send a test notification through a channel",
    description = "Editor role on the endpoint. Queues one `test` delivery for the channel and returns it; the send runs afterwards and the delivery's `state` moves on as it runs. At most 5 test deliveries per channel a minute; beyond that 429 `rate_limited` with `Retry-After: 60`. Bearer tokens are refused with 403 `session_required`.",
    security(("session_cookie" = []), ()),
    params(
        EndpointIdParam,
        ("channel_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Channel id, lowercase hyphenated UUID")
    ),
    responses(
        (status = 202, description = "The queued test delivery", body = DeliveryDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `forbidden`: viewer role; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`channel_not_found`: unknown channel of this endpoint; `not_found`: notifications are switched off, or an id is malformed", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 429, response = RateLimited),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn test(
    State(state): State<ApiState>,
    Path((raw_id, raw_channel)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    ensure_enabled(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let channel_id = channel_id(&raw_channel)?;
    let message = format!("Test notification from webhooks.live for endpoint {endpoint}.");
    match state
        .store
        .enqueue_test_delivery(&endpoint, state.settings.retention, channel_id, &message)
        .await?
    {
        TestOutcome::Queued(delivery) => {
            Ok((StatusCode::ACCEPTED, Json(DeliveryDto::from(&delivery))).into_response())
        }
        TestOutcome::EndpointGone => Err(ApiError::EndpointGone),
        TestOutcome::ChannelNotFound => Err(ApiError::ChannelNotFound),
        TestOutcome::Throttled => Err(ApiError::RateLimited {
            retry_after_secs: TEST_RETRY_AFTER_SECS,
        }),
    }
}

pub(crate) async fn resolve_config(
    state: &ApiState,
    keyring: &Keyring,
    endpoint: &EndpointId,
    channel: Uuid,
    caller: &Caller,
    parsed: &mut ParsedChannel,
) -> Result<Option<ConfigChange>, ApiError> {
    let (Some(kind), Some(config)) = (parsed.kind, parsed.config.take()) else {
        return Ok(None);
    };
    let stored = match config {
        ChannelConfig::Url(url) => {
            if kind == ChannelKind::Webhook {
                check_webhook(state, &url, &mut parsed.violations).await;
            }
            if !parsed.violations.is_empty() {
                return Ok(None);
            }
            let secret = Secret::new(url.clone()).map_err(|_| {
                ApiError::Validation(vec![FieldViolation::new(
                    "config.url",
                    FieldCode::InvalidFormat,
                    "is not a usable value",
                )])
            })?;
            let name = channel_secret_name(channel)
                .map_err(|_| ApiError::InternalFault("a channel secret name could not be built"))?;
            let sealed = keyring
                .seal(endpoint, &name, &secret)
                .map_err(ApiError::SecretSeal)?;
            ConfigChange {
                config: json!({ "url": mask_url(kind, &url) }),
                sealed: Some(sealed),
            }
        }
        ChannelConfig::Email { address_id } => {
            let user = signed_in_user(state, caller)?;
            let mut violations = email_violations(state, endpoint, user, address_id).await?;
            parsed.violations.append(&mut violations);
            if !parsed.violations.is_empty() {
                return Ok(None);
            }
            ConfigChange {
                config: json!({ "address_id": address_id.hyphenated().to_string() }),
                sealed: None,
            }
        }
        ChannelConfig::Telegram {
            token_secret,
            chat_id,
        } => {
            let exists = state
                .store
                .get_sealed_secret(endpoint, &token_secret)
                .await?
                .is_some();
            if !exists {
                parsed.violations.push(token_secret_violation());
                return Ok(None);
            }
            ConfigChange {
                config: json!({ "token_secret": token_secret.unpack(), "chat_id": chat_id }),
                sealed: None,
            }
        }
        ChannelConfig::Push => {
            if !parsed.violations.is_empty() {
                return Ok(None);
            }
            ConfigChange {
                config: json!({}),
                sealed: None,
            }
        }
    };
    Ok(Some(stored))
}

fn token_secret_violation() -> FieldViolation {
    FieldViolation::new(
        "config.token_secret",
        FieldCode::InvalidFormat,
        "names no secret of this endpoint",
    )
}

async fn email_violations(
    state: &ApiState,
    endpoint: &EndpointId,
    user: Uuid,
    address: Uuid,
) -> Result<Vec<FieldViolation>, ApiError> {
    let mut violations = Vec::new();
    if state.email.is_none() {
        violations.push(FieldViolation::new(
            "kind",
            FieldCode::NotAllowed,
            "email notifications are not configured on this instance",
        ));
        return Ok(violations);
    }
    match state
        .store
        .endpoint_is_owned(endpoint, state.settings.retention)
        .await?
    {
        Some(true) => {}
        Some(false) => violations.push(FieldViolation::new(
            "kind",
            FieldCode::NotAllowed,
            "email notifications need an endpoint with an owner",
        )),
        None => return Err(ApiError::EndpointGone),
    }
    if state
        .store
        .find_verified_address(user, address)
        .await?
        .is_none()
    {
        violations.push(FieldViolation::new(
            "config.address_id",
            FieldCode::NotAllowed,
            "names no verified email address of yours",
        ));
    }
    Ok(violations)
}

fn ensure_push_ready(state: &ApiState, caller: &Caller) -> Result<(), ApiError> {
    if state.push.is_none() {
        return Err(ApiError::PushDisabled);
    }
    if !state.access.has_users() {
        return Err(ApiError::NotAvailableInMode);
    }
    if caller.user().is_none() {
        return Err(ApiError::Auth(AuthRejection::SignInRequired));
    }
    Ok(())
}

async fn check_webhook(state: &ApiState, url: &str, violations: &mut Vec<FieldViolation>) {
    if targets_instance(url, &state.settings.public_url) {
        violations.push(FieldViolation::new(
            "config.url",
            FieldCode::TargetRefused,
            "must not point at this instance",
        ));
        return;
    }
    let vetted = vet_target(url, &state.forward.guard, state.forward.lookup.as_ref()).await;
    if let Err(error) = vetted {
        violations.push(target_violation_for("config.url", &error));
    }
}

fn ensure_enabled(state: &ApiState) -> Result<(), ApiError> {
    if state.notifications.enabled {
        Ok(())
    } else {
        Err(ApiError::NotFound)
    }
}

fn keyring(state: &ApiState) -> Result<&Keyring, ApiError> {
    state.secrets.as_deref().ok_or(ApiError::SecretsDisabled)
}

async fn live_endpoint(state: &ApiState, endpoint: &EndpointId) -> Result<(), ApiError> {
    state
        .store
        .get_endpoint(endpoint, state.settings.retention)
        .await?
        .map(|_| ())
        .ok_or(ApiError::EndpointGone)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}

fn channel_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw)
        .ok_or(ApiError::NotFound)
}
