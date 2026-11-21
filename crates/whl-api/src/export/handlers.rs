use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::http::header::{ALLOW, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use time::OffsetDateTime;
use whl_auth::{Access, Caller};
use whl_core::endpoint_id::EndpointId;
use whl_export::{ExportFormat, file_name};
use whl_store::{EndpointError, ExportSelection};

use super::params::{ExportParams, parse_selection};
use super::stream::{self, Job, MAX_EXPORT_BYTES};
use crate::body::RawBody;
use crate::error::ApiError;
use crate::state::ApiState;

const BUSY_RETRY_SECS: u64 = 1;
const ALLOWED_METHODS: &str = "GET, POST";

pub async fn head() -> Response {
    let mut response = ApiError::MethodNotAllowed.into_response();
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(ALLOWED_METHODS));
    response
}

pub async fn get(
    State(state): State<ApiState>,
    access: Access,
    Path(raw_id): Path<String>,
    Query(params): Query<ExportParams>,
) -> Result<Response, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let (format, selection) = params.parse()?;
    start(&state, access.caller(), endpoint, format, selection).await
}

pub async fn post(
    State(state): State<ApiState>,
    access: Access,
    Path(raw_id): Path<String>,
    RawBody(body): RawBody,
) -> Result<Response, ApiError> {
    let endpoint = endpoint_id(&raw_id)?;
    let (format, selection) = parse_selection(&body)?;
    start(&state, access.caller(), endpoint, format, selection).await
}

async fn start(
    state: &ApiState,
    caller: &Caller,
    endpoint: EndpointId,
    format: ExportFormat,
    selection: ExportSelection,
) -> Result<Response, ApiError> {
    let permit = state.exports.acquire(caller).ok_or(ApiError::RateLimited {
        retry_after_secs: BUSY_RETRY_SECS,
    })?;
    let record = state
        .store
        .get_endpoint(&endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)?;
    let snapshot = state.store.export_snapshot(&endpoint, &selection).await?;
    if snapshot.total_bytes > i64::try_from(MAX_EXPORT_BYTES).unwrap_or(i64::MAX) {
        return Err(ApiError::ExportTooLarge);
    }
    let exported_at = OffsetDateTime::now_utc();
    let name = file_name(endpoint.unpack(), exported_at, format);
    let disposition =
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")).map_err(|error| {
            ApiError::Internal(EndpointError::Invalid {
                id: endpoint.unpack().to_owned(),
                reason: format!("the export file name cannot be sent: {error}"),
            })
        })?;
    let body: Body = stream::body(Job {
        store: state.store.clone(),
        endpoint: record,
        selection,
        snapshot,
        format,
        exported_at,
        _permit: permit,
    });
    let mut response = Response::new(body);
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static(format.content_type()),
    );
    headers.insert(CONTENT_DISPOSITION, disposition);
    Ok(response)
}

fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}
