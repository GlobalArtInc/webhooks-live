use axum::Json;
use axum::extract::{Path, State};
use whl_capture::{TemplateDraft, render_preview};
use whl_core::response::ResponseConfig;
use whl_template::NameValue;

use super::dto::{DiagnosticDto, PreviewDto, PreviewInputDoc};
use super::input::parse_preview;
use super::subject::{blocking, capture_context, endpoint_id, live_endpoint, take_slot};
use crate::body::RawBody;
use crate::error::{ApiError, ErrorBody};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{
    BadRequest, PayloadTooLarge, RateLimited, StoreUnavailable, Unauthenticated,
    UnsupportedMediaType,
};
use crate::resolve::validate_response;
use crate::state::ApiState;

fn draft_of(config: &ResponseConfig, status_template: Option<String>) -> TemplateDraft {
    TemplateDraft {
        status: config.status().unpack(),
        status_template,
        content_type: config.content_type().unpack().to_owned(),
        headers: config
            .headers()
            .iter()
            .map(|header| NameValue::new(header.name.unpack(), header.value.unpack()))
            .collect(),
        body: config.body().to_vec(),
    }
}

#[utoipa::path(
    post,
    path = "/endpoints/{id}/template/preview",
    operation_id = "templating_preview",
    tag = "Templating",
    summary = "Render a draft response template against a stored capture",
    description = "Renders the draft `response` (`status`, `content_type`, `headers`, `body`, optional `status_template`, the shapes of `EndpointInput`) as a templated response, with the engine, limits and context the live responder uses: 100 ms in total, 50 ms per template, body 1 MiB, header value 1024 bytes, secrets refused. Nothing is sent, stored or counted, and persistent variables are only read. `capture_id` names a capture of this endpoint; `null` or absent checks the draft without rendering. `rendered` is null when any diagnostic has severity `error` or no capture was named; a header that renders an invalid value is dropped with a `warning`, as live. Diagnostics carry `field` (`response.body`, `response.headers.N.value`, `response.status_template`, or a plain response field such as `response.headers.N.name` when the draft itself is invalid), `code`, `severity`, and `line`, `column`, `length` (1-based, in characters) of the offending text. Viewer role (viewers see the editor read-only and can still preview); session only: a Bearer token answers 403 `session_required`. At most 8 previews and catalogue requests run at once on a replica.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam),
    request_body(content = PreviewInputDoc, content_type = "application/json"),
    responses(
        (status = 200, description = "The diagnostics and, when the draft is clean, the rendered response", body = PreviewDto),
        (status = 400, response = BadRequest),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used; `csrf_rejected`: the Origin check failed", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id; `capture_not_found`: no such capture on this endpoint", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 413, response = PayloadTooLarge),
        (status = 415, response = UnsupportedMediaType),
        (status = 422, description = "`validation_failed` (see `error.fields`): `capture_id` must be a capture id, `response` is required and takes only `status`, `content_type`, `headers`, `body` and `status_template` of the endpoint input; unknown fields are `not_allowed`", body = ErrorBody),
        (status = 429, response = RateLimited),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn preview(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Json<PreviewDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let request = parse_preview(&body)?;
    let record = live_endpoint(&state, &endpoint).await?;
    let capture_id = request.capture.as_ref().map(ToString::to_string);
    let context = match &request.capture {
        Some(capture) => Some(capture_context(&state, &endpoint, &record, capture).await?),
        None => None,
    };
    let config = match validate_response(&ResponseConfig::default(), &request.response) {
        Ok(config) => config,
        Err(error) => {
            return Ok(Json(PreviewDto::new(
                capture_id,
                None,
                DiagnosticDto::from_core(&error),
            )));
        }
    };
    let status_template = request.response.status_template.flatten();
    let draft = draft_of(&config, status_template);
    let slot = take_slot(&state)?;
    let report = blocking(slot, move || render_preview(&draft, context.as_ref())).await?;
    let diagnostics = report.diagnostics.iter().map(DiagnosticDto::from).collect();
    Ok(Json(PreviewDto::new(
        capture_id,
        report.rendered.as_ref(),
        diagnostics,
    )))
}
