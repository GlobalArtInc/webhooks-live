use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use whl_auth::{AuthRejection, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_forward::vet_target;
use whl_notify::{channel_secret_name, mask_url, targets_instance};
use whl_secrets::{Keyring, Secret};
use whl_store::Uuid;
use whl_store::notify::{
    ChannelKind, ChannelPatch, ConfigChange, CreateChannelOutcome, DeleteChannelOutcome,
    NewChannel, TestOutcome, Trigger, UpdateChannelOutcome,
};

use super::dto::{ChannelDto, ChannelListDto, DeliveryDto};
use super::email::signed_in_user;
use super::input::{ChannelConfig, DEFAULT_PER_HOUR, ParsedChannel, parse_channel};
use crate::body::RawBody;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::forward::target_violation_for;
use crate::state::ApiState;

const TEST_RETRY_AFTER_SECS: u64 = 60;

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
    let channel = build_channel(&state, &caller, &endpoint, Uuid::now_v7(), &body).await?;
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
    }
}

pub(crate) async fn build_channel(
    state: &ApiState,
    caller: &Caller,
    endpoint: &EndpointId,
    id: Uuid,
    body: &[u8],
) -> Result<NewChannel, ApiError> {
    let keyring = keyring(state)?;
    let mut parsed = parse_channel(body, true, None)?;
    if parsed.kind == Some(ChannelKind::Push) {
        ensure_push_ready(state, caller)?;
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
    }
}

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
                parsed.violations.push(FieldViolation::new(
                    "config.token_secret",
                    FieldCode::InvalidFormat,
                    "names no secret of this endpoint",
                ));
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
