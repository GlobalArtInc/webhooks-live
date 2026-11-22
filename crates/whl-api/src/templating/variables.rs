use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use whl_template::{CatalogueBasis, catalogue};

use super::doc::VariablesQuery;
use super::dto::CatalogueDto;
use super::input::parse_capture_id;
use super::subject::{
    blocking, capture_context, endpoint_id, live_endpoint, take_slot, variables_only_context,
};
use crate::error::{ApiError, ErrorBody};
use crate::openapi::params::EndpointIdParam;
use crate::openapi::responses::{RateLimited, StoreUnavailable, Unauthenticated, ValidationFailed};
use crate::state::ApiState;

#[derive(Debug, Default, Deserialize)]
pub struct VariablesParams {
    capture_id: Option<String>,
}

#[utoipa::path(
    get,
    path = "/endpoints/{id}/template/variables",
    operation_id = "templating_variables",
    tag = "Templating",
    summary = "The variable catalogue of a capture",
    description = "The names a response template can read, as the template engine defines them, with the `insert_text` for each, a `sample` taken from the named capture and a `description`. `group` is `request` (request, capture and endpoint roots, plus the names and JSON body keys of the capture: JSON keys appear as `json(\"$.path\")` calls, at most 4 levels deep and 100 entries), `time`, `variables` (the persistent variables and counters of the endpoint) or `helpers`. `type` is a JSON type, `integer`, `time`, `json` or `function`. Without `capture_id` the fixed names, variables and helpers are listed without samples. `delimiters` are the template delimiters. Run variables (`var.*`), steps and secrets are not listed. Viewer role (viewers see the editor read-only and can still preview); session only: a Bearer token answers 403 `session_required`.",
    security(("session_cookie" = []), ()),
    params(EndpointIdParam, VariablesQuery),
    responses(
        (status = 200, description = "The catalogue", body = CatalogueDto),
        (status = 401, response = Unauthenticated),
        (status = 403, description = "`session_required`: a Bearer token was used", body = ErrorBody),
        (status = 404, description = "`not_found`: malformed endpoint id; `capture_not_found`: no such capture on this endpoint", body = ErrorBody),
        (status = 410, description = "`endpoint_gone`: unknown, expired or deleted endpoint, or no role on it in `sso`", body = ErrorBody),
        (status = 422, response = ValidationFailed),
        (status = 429, response = RateLimited),
        (status = 503, response = StoreUnavailable)
    )
)]
pub async fn variables(
    State(state): State<ApiState>,
    Path(raw_id): Path<String>,
    Query(params): Query<VariablesParams>,
) -> Result<Json<CatalogueDto>, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let capture = params
        .capture_id
        .as_deref()
        .map(parse_capture_id)
        .transpose()
        .map_err(|violation| ApiError::Validation(vec![violation]))?;
    let record = live_endpoint(&state, &endpoint).await?;
    let (context, basis) = match &capture {
        Some(capture) => (
            capture_context(&state, &endpoint, &record, capture).await?,
            CatalogueBasis::Capture,
        ),
        None => (
            variables_only_context(&state, &endpoint).await?,
            CatalogueBasis::NoCapture,
        ),
    };
    let slot = take_slot(&state)?;
    let entries = blocking(slot, move || catalogue(&context, basis)).await?;
    Ok(Json(CatalogueDto::new(&entries)))
}
