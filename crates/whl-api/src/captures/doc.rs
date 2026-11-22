#![allow(dead_code)]

use utoipa::{IntoParams, OpenApi};

use super::dto::{CaptureDto, CaptureInputDoc, ForwardRequestDoc, ListDto, SummaryDto};
use super::{annotate, body, handlers, parts, stats};
use crate::error::ErrorBody;
use crate::forward::JobDto;
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, StoreUnavailable, UnsupportedMediaType, ValidationFailed,
};

#[derive(IntoParams)]
#[into_params(parameter_in = Path)]
pub struct CaptureIdParam {
    #[param(
        example = "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21",
        min_length = 36,
        max_length = 36,
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$"
    )]
    capture_id: String,
}

#[derive(IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CaptureListQuery {
    order: Option<String>,
    #[param(minimum = 1, maximum = 200)]
    limit: Option<i64>,
    cursor: Option<String>,
    method: Option<String>,
    status: Option<String>,
    kind: Option<String>,
    starred: Option<String>,
    #[param(min_length = 1, max_length = 200)]
    q: Option<String>,
    #[param(minimum = 0)]
    after_seq: Option<i64>,
    #[param(format = DateTime)]
    since: Option<String>,
    #[param(format = DateTime)]
    until: Option<String>,
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/captures/{capture_id}/forward",
    operation_id = "captures_forward",
    tag = "Captures",
    summary = "Forward a capture to a target",
    description = "Queues a manual forward of the capture and answers 202 with the job, `state` `pending`. The body is either `rule_id` alone, which uses that forward rule's settings, or `target_url` with optional `method`, `content_type` and `header_allowlist`; absent or null means the captured value. An empty body answers 422 `target_url` required. The target is checked as for forward rules (field codes `target_refused`, `target_unresolvable`, `invalid_format`); every attempt and redirect is checked again. Answers 404 `not_found` while forwarding is disabled. Needs an editor; a token needs the `send` scope.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
        CaptureIdParam,
    ),
    request_body(content = ForwardRequestDoc, content_type = "application/json"),
    responses(
        (status = 202, description = "The queued forward job", body = JobDto),
        (status = 400, response = BadRequest),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`forbidden`: the role is below editor; `insufficient_scope`: the token lacks `send` or is limited to other endpoints; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`capture_not_found`; `forward_rule_not_found` (unknown `rule_id`); `not_found`: malformed id, or forwarding is disabled", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
fn captures_forward() {}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/captures/{capture_id}/resend",
    operation_id = "captures_resend",
    tag = "Captures",
    summary = "Resend a capture to a target",
    description = "Same route handler and answers as `POST .../captures/{capture_id}/forward`, with the same body (`rule_id` or `target_url`) and the same status codes. Both paths are kept.",
    security(("session_cookie" = []), ("api_token" = [])),
    params(
        EndpointIdParam,
        CaptureIdParam,
    ),
    request_body(content = ForwardRequestDoc, content_type = "application/json"),
    responses(
        (status = 202, description = "The queued forward job", body = JobDto),
        (status = 400, response = BadRequest),
        (status = 401, description = "`unauthenticated` or `invalid_token`; `grant_required` for a view-protected anonymous endpoint without a valid grant", body = ErrorBody),
        (status = 403, description = "`forbidden`: the role is below editor; `insufficient_scope`: the token lacks `send` or is limited to other endpoints; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`capture_not_found`; `forward_rule_not_found` (unknown `rule_id`); `not_found`: malformed id, or forwarding is disabled", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, response = ValidationFailed),
        (status = 503, response = StoreUnavailable)
    )
)]
fn captures_resend() {}

#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::list,
        handlers::latest,
        handlers::show,
        handlers::delete,
        handlers::clear,
        annotate::update,
        body::download,
        parts::download,
        stats::show,
        captures_forward,
        captures_resend,
        crate::rules::runs::for_capture,
        crate::rules::runs::rerun,
    ),
    components(schemas(SummaryDto, ListDto, CaptureDto, CaptureInputDoc, ForwardRequestDoc, stats::StatsDto)),
    tags((name = "Captures", description = "Captured requests of an endpoint: list, read, annotate, delete, download, forward and statistics"))
)]
pub struct CapturesDoc;
