use std::collections::BTreeMap;

use serde_json::Value;
use tokio::sync::OwnedSemaphorePermit;
use whl_capture::{RuleFacts, persistent_context, response_context};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_store::EndpointRecord;
use whl_template::Context;

use crate::error::ApiError;
use crate::state::ApiState;

pub(super) fn endpoint_id(raw: &str) -> Result<EndpointId, ApiError> {
    EndpointId::parse(raw).map_err(|_| ApiError::NotFound)
}

pub(super) async fn live_endpoint(
    state: &ApiState,
    endpoint: &EndpointId,
) -> Result<EndpointRecord, ApiError> {
    state
        .store
        .get_endpoint(endpoint, state.settings.retention)
        .await?
        .ok_or(ApiError::EndpointGone)
}

pub(super) async fn persistent_values(
    state: &ApiState,
    endpoint: &EndpointId,
) -> Result<BTreeMap<String, Value>, ApiError> {
    let records = state.store.list_vars(endpoint).await?;
    Ok(records
        .into_iter()
        .map(|record| (record.name, record.value))
        .collect())
}

pub(super) async fn capture_context(
    state: &ApiState,
    endpoint: &EndpointId,
    record: &EndpointRecord,
    capture: &CaptureId,
) -> Result<Context, ApiError> {
    let detail = state
        .store
        .get_capture(endpoint, capture)
        .await?
        .ok_or(ApiError::CaptureNotFound)?;
    let facts = RuleFacts::from_detail(
        &detail,
        &state.settings.public_url,
        record.settings.title.as_deref(),
    )
    .map_err(|_| ApiError::InternalFault("a stored capture time cannot be read"))?;
    let persistent = persistent_values(state, endpoint).await?;
    Ok(response_context(facts, &persistent))
}

pub(super) async fn variables_only_context(
    state: &ApiState,
    endpoint: &EndpointId,
) -> Result<Context, ApiError> {
    let persistent = persistent_values(state, endpoint).await?;
    Ok(persistent_context(&persistent))
}

pub(super) fn take_slot(state: &ApiState) -> Result<OwnedSemaphorePermit, ApiError> {
    state.previews.acquire().ok_or(ApiError::RateLimited {
        retry_after_secs: 1,
    })
}

pub(super) async fn blocking<T, F>(permit: OwnedSemaphorePermit, job: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        job()
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "a template job failed");
        ApiError::InternalFault("the template job did not finish")
    })
}
