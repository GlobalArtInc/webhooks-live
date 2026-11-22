use std::time::Duration;

use axum::Json;
use axum::extract::State;
use whl_script::{
    AFTER_RESPONSE_TIME, BEFORE_RESPONSE_TIME, DEFAULT_MEMORY_BYTES, EngineLimits, RunLimits,
    ScriptCompiler,
};

use super::dto::{ScriptLimitsDto, ScriptMetaDto, ScriptPhaseLimitsDto, ScriptValidationDto};
use super::input::parse_validate;
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody};
use crate::openapi::responses::{
    BadRequest, InternalError, PayloadTooLarge, RateLimited, Unauthenticated, UnsupportedMediaType,
};
use crate::state::ApiState;

pub(crate) const SCRIPT_CHECK_SLOTS: usize = 4;
const COMPILE_DEADLINE: Duration = Duration::from_secs(2);

fn milliseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn limits_of(state: &ApiState) -> ScriptLimitsDto {
    let before = RunLimits::before_response();
    let after = RunLimits::after_response();
    let memory = state.rules.as_ref().map_or(DEFAULT_MEMORY_BYTES, |rules| {
        rules.settings().script_memory_bytes
    });
    ScriptLimitsDto::new(
        &EngineLimits::default(),
        memory,
        ScriptPhaseLimitsDto::new(before.max_operations, milliseconds(BEFORE_RESPONSE_TIME)),
        ScriptPhaseLimitsDto::new(after.max_operations, milliseconds(AFTER_RESPONSE_TIME)),
    )
}

#[utoipa::path(
    get,
    path = "/script/meta",
    operation_id = "script_meta",
    tag = "Script",
    summary = "The catalogue of the script step language",
    description = "What the script editor offers: `name` of the language (Rhai), `keywords`, `functions` (`name`, `signature`, a one-line `doc`, `parameters` with `name` and `type`, and `returns`, null when nothing is returned), `variables` (the names a script can read, with dotted paths such as `request.method`, a `type` and a `doc`) and the `limits` that stop a script: source size, memory, and per phase (`before_response`, `after_response`) the operation count and the time. The time is the longest a single run may take in that phase (100 ms before the response, 1 s after it, the engine's ceilings for every tier); a run also stops earlier when the rest of the response budget is shorter. The lists are the engine's own tables, checked against its registered functions by tests, so they cannot drift. `reference_url` is the reference page of the language. Needs no endpoint: any caller who may use the application (in `sso` a signed-in one) may read it; session only: a Bearer token answers 403 `session_required`.",
    security(("session_cookie" = []), ()),
    responses(
        (status = 200, description = "The language catalogue", body = ScriptMetaDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used", body = ErrorBody),
        (status = 429, response = RateLimited)
    )
)]
pub async fn meta(State(state): State<ApiState>) -> Json<ScriptMetaDto> {
    Json(ScriptMetaDto::new(limits_of(&state)))
}

#[utoipa::path(
    post,
    path = "/script/validate",
    operation_id = "script_validate",
    tag = "Script",
    summary = "Check a script source without running it",
    description = "Compiles `source` (at most 16 KiB; `phase` is `before` or `after` and is accepted for the editor, compiling does not depend on it) with the engine and limits a rule save uses, and never runs it. A source that does not compile is a 200 whose `diagnostics` list each problem with `severity` (always `error`), `code` (`syntax`, `unexpected_end`, `disabled_feature`, `expression_too_deep`, `too_many_functions`, `literal_too_large`, `undefined_variable`, `assignment_to_constant`, `invalid_function`, `invalid_literal`), `message`, and `line`, `column` and `length` (1-based, in characters) of the offending text; a clean source answers an empty list. A source over 16 KiB answers 422 `validation_failed` (field `source`, code `too_long`). Needs no endpoint: any caller who may use the application (in `sso` a signed-in one); session only: a Bearer token answers 403 `session_required`. At most 4 checks run at once on a replica, more answer 429 `rate_limited` with `Retry-After: 1`; a check that does not finish within 2 seconds answers 500 `internal_error`.",
    security(("session_cookie" = []), ()),
    request_body(content = super::dto::ScriptValidateInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The diagnostics, empty when the source compiles", body = ScriptValidationDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`): `source` is required, a string of at most 16 KiB; `phase` is `before` or `after`; unknown fields are `not_allowed`", body = ErrorBody),
        (status = 429, response = RateLimited),
        (status = 500, response = InternalError)
    )
)]
pub async fn validate(
    State(state): State<ApiState>,
    RawBody(body): RawBody,
) -> Result<Json<ScriptValidationDto>, ApiError> {
    let request = parse_validate(&body)?;
    let compiler = ScriptCompiler::new(EngineLimits::default()).map_err(ApiError::RuleEngine)?;
    let permit = state.script_checks.acquire().ok_or(ApiError::RateLimited {
        retry_after_secs: 1,
    })?;
    let job = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        compiler.compile(&request.source).map(|_| ())
    });
    let finished = tokio::time::timeout(COMPILE_DEADLINE, job)
        .await
        .map_err(|_| {
            tracing::error!("a script check did not finish in time");
            ApiError::InternalFault("the script check did not finish in time")
        })?
        .map_err(|error| {
            tracing::error!(%error, "a script check failed");
            ApiError::InternalFault("the script check did not finish")
        })?;
    Ok(Json(match finished {
        Ok(()) => ScriptValidationDto::clean(),
        Err(error) => ScriptValidationDto::from_error(&error),
    }))
}
