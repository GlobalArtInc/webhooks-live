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

use super::dto::{QueuedDto, RunDto, RunListDto, ScheduleDto, ScheduleListDto};
use super::input::{ParsedSchedule, parse_schedule};
use crate::body::RawBody;
use crate::dto::timestamp;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::state::ApiState;

const DEFAULT_TIMEZONE: &str = "UTC";

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
    let schedule = cron.as_deref().and_then(|cron| {
        check_cron(
            state.schedules.min_interval,
            cron,
            &timezone,
            now,
            &mut violations,
        )
    });
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
        .create_schedule(
            &endpoint,
            state.settings.retention,
            &new,
            state.schedules.max_per_user,
        )
        .await?
    {
        CreateScheduleOutcome::Created(created) => {
            tracing::info!(endpoint = %endpoint, schedule = %created.id, "schedule created");
            Ok((StatusCode::CREATED, Json(ScheduleDto::from(&*created))).into_response())
        }
        CreateScheduleOutcome::EndpointGone => Err(ApiError::EndpointGone),
        CreateScheduleOutcome::NoOwner => Err(ApiError::Auth(AuthRejection::ClaimRequired)),
        CreateScheduleOutcome::LimitReached => {
            Err(ApiError::ScheduleLimit(state.schedules.max_per_user))
        }
    }
}

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
    let min_interval = state.schedules.min_interval;
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

fn check_cron(
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
    let compiled = match CompiledRequest::compile(request) {
        Ok(compiled) => compiled,
        Err(found) => {
            violations.extend(found.iter().map(request_violation));
            return Ok(());
        }
    };
    check_secrets(state, endpoint, &compiled, violations).await?;
    if let Some(url) = &compiled.static_url {
        let vetted = vet_target(url, &state.forward.guard, state.forward.lookup.as_ref()).await;
        if let Err(error) = vetted {
            violations.push(target_violation(&error));
        }
    }
    Ok(())
}

async fn check_secrets(
    state: &ApiState,
    endpoint: &EndpointId,
    compiled: &CompiledRequest,
    violations: &mut Vec<FieldViolation>,
) -> Result<(), ApiError> {
    let mut wanted: Vec<(String, BTreeSet<String>)> =
        vec![("request.url".to_owned(), compiled.url.referenced_secrets())];
    for (index, header) in compiled.headers.iter().enumerate() {
        wanted.push((
            format!("request.headers.{index}.value"),
            header.value.referenced_secrets(),
        ));
    }
    if wanted.iter().all(|(_, names)| names.is_empty()) {
        return Ok(());
    }
    let known: BTreeSet<String> = if state.secrets_enabled() {
        state
            .store
            .list_secrets(endpoint)
            .await?
            .into_iter()
            .map(|meta| meta.name.unpack().to_owned())
            .collect()
    } else {
        BTreeSet::new()
    };
    for (field, names) in wanted {
        for name in names.iter().filter(|name| !known.contains(*name)) {
            violations.push(FieldViolation::new(
                field.clone(),
                FieldCode::SecretUnknown,
                format!("secret {name} does not exist on this endpoint"),
            ));
        }
    }
    Ok(())
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
