use std::collections::BTreeSet;
use std::time::Duration;

use axum::Json;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use time::OffsetDateTime;
use whl_auth::{Access, AuthRejection, Caller, EndpointRole};
use whl_core::endpoint_id::EndpointId;
use whl_forward::{RefusalReason, TargetCheckError, vet_target};
use whl_schedule::{
    CompiledRequest, CronError, CronSchedule, Violation, ViolationKind, request_uses_secrets,
};
use whl_store::Uuid;
use whl_store::schedules::{
    CreateScheduleOutcome, DeleteScheduleOutcome, NewSchedule, PatchScheduleOutcome,
    QueueRunOutcome, ScheduleRecord, ScheduleRequest, ScheduleUpdate,
};

use super::doc::ScheduleInputDoc;
use super::dto::{QueuedDto, RunDto, RunListDto, ScheduleDto, ScheduleListDto};
use super::input::{ParsedSchedule, parse_schedule};
use crate::body::RawBody;
use crate::dto::timestamp;
use crate::error::{ApiError, ErrorBody, FieldCode, FieldViolation};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, Unauthenticated, UnsupportedMediaType, ValidationFailed,
};
use crate::state::ApiState;

const DEFAULT_TIMEZONE: &str = "UTC";

#[utoipa::path(
    get,
    path = "/endpoints/{id}/schedules",
    operation_id = "schedules_list",
    tag = "Schedules",
    summary = "List the schedules of an endpoint",
    description = "Lists every schedule of the endpoint, enabled or paused, oldest first. Needs the viewer role on the endpoint. `open` answers 403 `not_available_in_mode`; a token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(EndpointIdParam),
    responses(
        (status = 200, description = "The schedules", body = ScheduleListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: schedules are off on the instance; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
pub async fn list(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
) -> Result<Json<ScheduleListDto>, ApiError> {
    ensure_available(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let schedules = state.store.list_schedules(&endpoint).await?;
    Ok(Json(ScheduleListDto {
        items: schedules.iter().map(ScheduleDto::from).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/schedules",
    operation_id = "schedules_create",
    tag = "Schedules",
    summary = "Create a schedule",
    description = "Needs the owner role on the endpoint and a signed-in caller; the schedule is owned by the caller. `name`, `cron` and `request` are required. `cron` has five fields and may not fire more often than the tier allows (`interval_too_short`); `timezone` is an IANA name, default `UTC`. `request.method` is one of `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`; `request.url` is `http` or `https` and may name endpoint secrets as `secret.<NAME>` (`secret_unknown` when missing); `request.headers` is optional, at most 20 entries (names up to 64 bytes, values up to 1024), and may use secrets too; `request.body` is optional, at most 64 KiB, and is a template. A target that fails the SSRF rules answers 422 `target_refused`. `enabled` defaults to true; an enabled schedule gets `next_run_at`. A token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(EndpointIdParam),
    request_body(content = ScheduleInputDoc, content_type = "application/json"),
    responses(
        (status = 201, description = "The schedule", body = ScheduleDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: not the owner; `sign_in_required`: `mixed` without a session; `session_required`: a Bearer token was used; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id", body = ErrorBody),
        (status = 409, description = "`claim_required`: `mixed` endpoint with no owner yet", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`; codes such as `cron_invalid`, `timezone_invalid`, `interval_too_short`, `secret_unknown`, `target_refused`) or `limit_reached` (`error.limit` = `{name: \"schedules\", max}`)", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: schedules are off on the instance; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
pub async fn create(
    State(state): State<ApiState>,
    caller: Caller,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    ensure_available(&state)?;
    let user = caller
        .user()
        .ok_or(ApiError::Auth(AuthRejection::SignInRequired))?;
    let endpoint = endpoint_id(&raw_id)?;
    live_endpoint(&state, &endpoint).await?;
    let ParsedSchedule {
        name,
        cron,
        timezone,
        request,
        enabled,
        mut violations,
    } = parse_schedule(&body, true)?;
    let now = OffsetDateTime::now_utc();
    let timezone = timezone.unwrap_or_else(|| DEFAULT_TIMEZONE.to_owned());
    let tier = state.endpoint_tier(&endpoint).await?;
    let min_interval = tier.limits().schedule_min_interval;
    let max_per_user = usize::try_from(tier.limits().schedules).unwrap_or(usize::MAX);
    let schedule = cron
        .as_deref()
        .and_then(|cron| check_cron(min_interval, cron, &timezone, now, &mut violations));
    if let Some(request) = &request {
        check_request(&state, &endpoint, request, &mut violations).await?;
    }
    let (Some(name), Some(cron), Some(request), Some(schedule)) = (name, cron, request, schedule)
    else {
        return Err(ApiError::Validation(violations));
    };
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    let enabled = enabled.unwrap_or(true);
    let next_run_at = if enabled {
        Some(first_run(&schedule, now)?)
    } else {
        None
    };
    let new = NewSchedule {
        created_by: user.id,
        name,
        cron,
        timezone,
        request,
        enabled,
        next_run_at,
    };
    match state
        .store
        .create_schedule(&endpoint, state.settings.retention, &new, max_per_user)
        .await?
    {
        CreateScheduleOutcome::Created(created) => {
            tracing::info!(endpoint = %endpoint, schedule = %created.id, "schedule created");
            Ok((StatusCode::CREATED, Json(ScheduleDto::from(&*created))).into_response())
        }
        CreateScheduleOutcome::EndpointGone => Err(ApiError::EndpointGone),
        CreateScheduleOutcome::NoOwner => Err(ApiError::Auth(AuthRejection::ClaimRequired)),
        CreateScheduleOutcome::LimitReached => Err(ApiError::ScheduleLimit(max_per_user)),
    }
}

#[utoipa::path(
    patch,
    path = "/endpoints/{id}/schedules/{schedule_id}",
    operation_id = "schedules_update",
    tag = "Schedules",
    summary = "Change a schedule",
    description = "A partial ScheduleInput: absent fields are unchanged; the same rules as create apply to the fields sent. Needs the editor role on the endpoint. Only the owner of the schedule may change `request` when the old or the new request uses secrets (403 `owner_required`). `enabled: true` clears a pause (`paused_reason`) and resets `consecutive_failures`. Changing `cron` or `timezone` recomputes `next_run_at`. An empty body is 400 `invalid_json`. A token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(EndpointIdParam, ("schedule_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Schedule id, lowercase hyphenated UUID")),
    request_body(content = ScheduleInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The updated schedule", body = ScheduleDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: below the editor role; `owner_required`: a non-owner changed a request that uses secrets; `session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`schedule_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, response = ValidationFailed),
        (status = 503, description = "`feature_disabled`: schedules are off on the instance; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
pub async fn update(
    State(state): State<ApiState>,
    access: Option<Extension<Access>>,
    Path((raw_id, raw_schedule)): Path<(String, String)>,
    RawBody(body): RawBody,
) -> Result<Json<ScheduleDto>, ApiError> {
    ensure_available(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let schedule_id = schedule_id(&raw_schedule)?;
    live_endpoint(&state, &endpoint).await?;
    let ParsedSchedule {
        name,
        cron,
        timezone,
        request,
        enabled,
        violations,
    } = parse_schedule(&body, false)?;
    let mut violations = violations;
    if let Some(request) = &request {
        check_request(&state, &endpoint, request, &mut violations).await?;
    }
    let owner = access.and_then(|Extension(access)| access.role()) == Some(EndpointRole::Owner);
    let patch = PatchFields {
        name,
        cron,
        timezone,
        request,
        enabled,
    };
    let min_interval = state
        .endpoint_tier(&endpoint)
        .await?
        .limits()
        .schedule_min_interval;
    let now = OffsetDateTime::now_utc();
    match state
        .store
        .patch_schedule(
            &endpoint,
            state.settings.retention,
            schedule_id,
            move |existing| plan_update(existing, patch, violations, owner, min_interval, now),
        )
        .await?
    {
        PatchScheduleOutcome::Updated(updated) => Ok(Json(ScheduleDto::from(&*updated))),
        PatchScheduleOutcome::EndpointGone => Err(ApiError::EndpointGone),
        PatchScheduleOutcome::NotFound => Err(ApiError::ScheduleNotFound),
        PatchScheduleOutcome::Refused(error) => Err(error),
    }
}

struct PatchFields {
    name: Option<String>,
    cron: Option<String>,
    timezone: Option<String>,
    request: Option<ScheduleRequest>,
    enabled: Option<bool>,
}

fn plan_update(
    existing: &ScheduleRecord,
    patch: PatchFields,
    mut violations: Vec<FieldViolation>,
    owner: bool,
    min_interval: Duration,
    now: OffsetDateTime,
) -> Result<ScheduleUpdate, ApiError> {
    if let Some(request) = &patch.request
        && *request != existing.request
        && !owner
        && (request_uses_secrets(request) || request_uses_secrets(&existing.request))
    {
        return Err(ApiError::ScheduleOwnerRequired);
    }
    let changed_timing = patch
        .cron
        .as_ref()
        .is_some_and(|cron| *cron != existing.cron)
        || patch
            .timezone
            .as_ref()
            .is_some_and(|timezone| *timezone != existing.timezone);
    let final_enabled = patch.enabled.unwrap_or(existing.enabled);
    let final_cron = patch.cron.unwrap_or_else(|| existing.cron.clone());
    let final_timezone = patch.timezone.unwrap_or_else(|| existing.timezone.clone());
    let schedule = if final_enabled || changed_timing {
        check_cron(
            min_interval,
            &final_cron,
            &final_timezone,
            now,
            &mut violations,
        )
    } else {
        None
    };
    if !violations.is_empty() {
        return Err(ApiError::Validation(violations));
    }
    let next_run_at = next_run_for_update(
        existing,
        schedule.as_ref(),
        final_enabled,
        changed_timing,
        now,
    )?;
    Ok(ScheduleUpdate {
        name: patch.name.unwrap_or_else(|| existing.name.clone()),
        cron: final_cron,
        timezone: final_timezone,
        request: patch.request.unwrap_or_else(|| existing.request.clone()),
        enabled: final_enabled,
        next_run_at,
        reset_failures: final_enabled && !existing.enabled,
        release_lease: changed_timing || final_enabled != existing.enabled,
    })
}

#[utoipa::path(
    delete,
    path = "/endpoints/{id}/schedules/{schedule_id}",
    operation_id = "schedules_delete",
    tag = "Schedules",
    summary = "Delete a schedule",
    description = "Needs the editor role on the endpoint. A token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(EndpointIdParam, ("schedule_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Schedule id, lowercase hyphenated UUID")),
    responses(
        (status = 204, description = "The schedule is deleted"),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: below the editor role; `session_required`: a Bearer token was used; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`schedule_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: schedules are off on the instance; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
pub async fn delete(
    State(state): State<ApiState>,
    Path((raw_id, raw_schedule)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    ensure_available(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let schedule_id = schedule_id(&raw_schedule)?;
    match state
        .store
        .delete_schedule(&endpoint, state.settings.retention, schedule_id)
        .await?
    {
        DeleteScheduleOutcome::Deleted => Ok(StatusCode::NO_CONTENT),
        DeleteScheduleOutcome::EndpointGone => Err(ApiError::EndpointGone),
        DeleteScheduleOutcome::NotFound => Err(ApiError::ScheduleNotFound),
    }
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/schedules/{schedule_id}/run",
    operation_id = "schedules_run",
    tag = "Schedules",
    summary = "Queue a run now",
    description = "Queues one run of the schedule, ahead of its cron time, and answers 202 with the time it was queued. A disabled schedule answers 409 `schedule_disabled`. Needs the editor role on the endpoint. A token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(EndpointIdParam, ("schedule_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Schedule id, lowercase hyphenated UUID")),
    responses(
        (status = 202, description = "The run is queued", body = QueuedDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`forbidden`: below the editor role; `session_required`: a Bearer token was used; `not_available_in_mode`: the instance has no accounts; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`schedule_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 409, description = "`schedule_disabled`: the schedule is disabled", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: schedules are off on the instance; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
pub async fn run(
    State(state): State<ApiState>,
    Path((raw_id, raw_schedule)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    ensure_available(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let schedule_id = schedule_id(&raw_schedule)?;
    let now = OffsetDateTime::now_utc();
    match state
        .store
        .queue_schedule_run(&endpoint, state.settings.retention, schedule_id, now)
        .await?
    {
        QueueRunOutcome::Queued(queued_at) => Ok((
            StatusCode::ACCEPTED,
            Json(QueuedDto {
                schedule_id: schedule_id.hyphenated().to_string(),
                queued_at: timestamp(queued_at),
            }),
        )
            .into_response()),
        QueueRunOutcome::EndpointGone => Err(ApiError::EndpointGone),
        QueueRunOutcome::NotFound => Err(ApiError::ScheduleNotFound),
        QueueRunOutcome::Disabled => Err(ApiError::ScheduleDisabled),
    }
}

#[utoipa::path(
    get,
    path = "/endpoints/{id}/schedules/{schedule_id}/runs",
    operation_id = "schedules_list_runs",
    tag = "Schedules",
    summary = "List the recent runs of a schedule",
    description = "The 50 most recent runs, newest first. `outcome` is `succeeded`, `failed` or `skipped`; `status` is the HTTP status when the request got one; `capture_id` is set when the run stored a capture. Needs the viewer role on the endpoint. A token is refused with 403 `session_required`.",
    security(("session_cookie" = [])),
    params(EndpointIdParam, ("schedule_id" = String, Path, pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$", description = "Schedule id, lowercase hyphenated UUID")),
    responses(
        (status = 200, description = "The runs", body = RunListDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `not_available_in_mode`: the instance has no accounts", body = ErrorBody),
        (status = 404, description = "`schedule_not_found`: unknown in this endpoint; `not_found`: malformed id", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 503, description = "`feature_disabled`: schedules are off on the instance; `store_unavailable`: the database timed out or is down (`Retry-After: 5`)", body = ErrorBody)
    )
)]
pub async fn runs(
    State(state): State<ApiState>,
    Path((raw_id, raw_schedule)): Path<(String, String)>,
) -> Result<Json<RunListDto>, ApiError> {
    ensure_available(&state)?;
    let endpoint = endpoint_id(&raw_id)?;
    let schedule_id = schedule_id(&raw_schedule)?;
    live_endpoint(&state, &endpoint).await?;
    let runs = state
        .store
        .list_schedule_runs(&endpoint, schedule_id)
        .await?
        .ok_or(ApiError::ScheduleNotFound)?;
    Ok(Json(RunListDto {
        items: runs.iter().map(RunDto::from).collect(),
    }))
}

fn next_run_for_update(
    existing: &ScheduleRecord,
    schedule: Option<&CronSchedule>,
    final_enabled: bool,
    changed_timing: bool,
    now: OffsetDateTime,
) -> Result<Option<OffsetDateTime>, ApiError> {
    if !final_enabled {
        return Ok(None);
    }
    let keep = existing.enabled && !changed_timing;
    match (keep, existing.next_run_at, schedule) {
        (true, Some(next), _) => Ok(Some(next)),
        (_, _, Some(schedule)) => Ok(Some(first_run(schedule, now)?)),
        _ => Ok(None),
    }
}

fn first_run(schedule: &CronSchedule, now: OffsetDateTime) -> Result<OffsetDateTime, ApiError> {
    schedule
        .next_after(now)
        .map_err(|error| ApiError::Validation(vec![cron_violation(&error)]))
}

pub(crate) fn check_cron(
    min_interval: Duration,
    cron: &str,
    timezone: &str,
    now: OffsetDateTime,
    violations: &mut Vec<FieldViolation>,
) -> Option<CronSchedule> {
    let checked = CronSchedule::parse(cron, timezone).and_then(|schedule| {
        schedule
            .check_interval(now, min_interval)
            .map(|()| schedule)
    });
    match checked {
        Ok(schedule) => Some(schedule),
        Err(error) => {
            violations.push(cron_violation(&error));
            None
        }
    }
}

fn cron_violation(error: &CronError) -> FieldViolation {
    match error {
        CronError::UnknownTimezone => FieldViolation::new(
            "timezone",
            FieldCode::TimezoneInvalid,
            "must be an IANA time zone name",
        ),
        CronError::IntervalTooShort { minimum, .. } => FieldViolation::new(
            "cron",
            FieldCode::IntervalTooShort,
            format!("fires more often than every {} seconds", minimum.as_secs()),
        ),
        other => FieldViolation::new("cron", FieldCode::CronInvalid, other.to_string()),
    }
}

async fn check_request(
    state: &ApiState,
    endpoint: &EndpointId,
    request: &ScheduleRequest,
    violations: &mut Vec<FieldViolation>,
) -> Result<(), ApiError> {
    let known = known_secrets(state, endpoint, request).await?;
    check_request_with(state, &known, request, violations).await;
    Ok(())
}

pub(crate) async fn check_request_with(
    state: &ApiState,
    known: &BTreeSet<String>,
    request: &ScheduleRequest,
    violations: &mut Vec<FieldViolation>,
) {
    let compiled = match CompiledRequest::compile(request) {
        Ok(compiled) => compiled,
        Err(found) => {
            violations.extend(found.iter().map(request_violation));
            return;
        }
    };
    check_secrets(known, &compiled, violations);
    if let Some(url) = &compiled.static_url {
        let vetted = vet_target(url, &state.forward.guard, state.forward.lookup.as_ref()).await;
        if let Err(error) = vetted {
            violations.push(target_violation(&error));
        }
    }
}

async fn known_secrets(
    state: &ApiState,
    endpoint: &EndpointId,
    request: &ScheduleRequest,
) -> Result<BTreeSet<String>, ApiError> {
    let Ok(compiled) = CompiledRequest::compile(request) else {
        return Ok(BTreeSet::new());
    };
    if !compiled_uses_secrets(&compiled) || !state.secrets_enabled() {
        return Ok(BTreeSet::new());
    }
    Ok(state
        .store
        .list_secrets(endpoint)
        .await?
        .into_iter()
        .map(|meta| meta.name.unpack().to_owned())
        .collect())
}

fn compiled_uses_secrets(compiled: &CompiledRequest) -> bool {
    !compiled.url.referenced_secrets().is_empty()
        || compiled
            .headers
            .iter()
            .any(|header| !header.value.referenced_secrets().is_empty())
}

fn check_secrets(
    known: &BTreeSet<String>,
    compiled: &CompiledRequest,
    violations: &mut Vec<FieldViolation>,
) {
    let mut wanted: Vec<(String, BTreeSet<String>)> =
        vec![("request.url".to_owned(), compiled.url.referenced_secrets())];
    for (index, header) in compiled.headers.iter().enumerate() {
        wanted.push((
            format!("request.headers.{index}.value"),
            header.value.referenced_secrets(),
        ));
    }
    for (field, names) in wanted {
        for name in names.iter().filter(|name| !known.contains(*name)) {
            violations.push(FieldViolation::new(
                field.clone(),
                FieldCode::SecretUnknown,
                format!("secret {name} does not exist on this endpoint"),
            ));
        }
    }
}

fn request_violation(violation: &Violation) -> FieldViolation {
    let code = match violation.kind {
        ViolationKind::Required => FieldCode::Required,
        ViolationKind::InvalidFormat => FieldCode::InvalidFormat,
        ViolationKind::TooLong => FieldCode::TooLong,
        ViolationKind::TooMany => FieldCode::TooMany,
        ViolationKind::NotAllowed => FieldCode::NotAllowed,
        ViolationKind::TemplateInvalid => FieldCode::TemplateInvalid,
        ViolationKind::SecretNotAllowed => FieldCode::SecretNotAllowed,
    };
    FieldViolation::new(violation.field.clone(), code, violation.message.clone())
}

fn target_violation(error: &TargetCheckError) -> FieldViolation {
    match error {
        TargetCheckError::Refused(reason) => {
            FieldViolation::new("request.url", refusal_code(*reason), reason.to_string())
        }
        TargetCheckError::Unresolvable(detail) => {
            FieldViolation::new("request.url", FieldCode::TargetUnresolvable, detail.clone())
        }
    }
}

fn refusal_code(reason: RefusalReason) -> FieldCode {
    match reason {
        RefusalReason::InvalidUrl
        | RefusalReason::UrlTooLong
        | RefusalReason::Scheme
        | RefusalReason::MissingHost => FieldCode::InvalidFormat,
        RefusalReason::Userinfo
        | RefusalReason::ForbiddenHost
        | RefusalReason::ForbiddenPort
        | RefusalReason::RefusedAddress
        | RefusalReason::HttpsDowngrade => FieldCode::TargetRefused,
    }
}

fn ensure_available(state: &ApiState) -> Result<(), ApiError> {
    if !state.schedules.enabled {
        return Err(ApiError::SchedulesDisabled);
    }
    if state.access.has_users() {
        Ok(())
    } else {
        Err(ApiError::NotAvailableInMode)
    }
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

fn schedule_id(raw: &str) -> Result<Uuid, ApiError> {
    Uuid::try_parse(raw)
        .ok()
        .filter(|uuid| uuid.hyphenated().to_string() == raw)
        .ok_or(ApiError::NotFound)
}
