use std::time::Duration;

use sqlx::types::Uuid;
use whl_core::endpoint_id::EndpointId;

use crate::event::{JsonText, Outcome};
use crate::route::LiveState;
use crate::view;

pub const LOOKUP_TIMEOUT: Duration = Duration::from_secs(5);

fn encoded(encoded: Result<JsonText, serde_json::Error>, event: &'static str) -> Outcome {
    match encoded {
        Ok(data) => Outcome::Data(data),
        Err(error) => {
            tracing::error!(%error, event, "cannot encode a stream event");
            Outcome::Failed
        }
    }
}

pub(crate) async fn endpoint(state: &LiveState, endpoint: &EndpointId) -> Outcome {
    let read = tokio::time::timeout(
        LOOKUP_TIMEOUT,
        state
            .store
            .endpoint_view(endpoint, state.settings.retention),
    )
    .await;
    match read {
        Ok(Ok(Some(found))) => encoded(
            view::endpoint_json(
                &found,
                &state.settings.public_url,
                state.settings.max_captures,
            ),
            "endpoint_updated",
        ),
        Ok(Ok(None)) => Outcome::Gone,
        Ok(Err(error)) => {
            tracing::warn!(%error, endpoint_id = %endpoint, "cannot read the endpoint for an update event");
            Outcome::Failed
        }
        Err(_) => {
            tracing::warn!(endpoint_id = %endpoint, "reading the endpoint for an update event timed out");
            Outcome::Failed
        }
    }
}

pub(crate) async fn forward_result(
    state: &LiveState,
    endpoint: &EndpointId,
    capture_id: &Uuid,
    job_id: &Uuid,
) -> Outcome {
    let read = tokio::time::timeout(
        LOOKUP_TIMEOUT,
        state.store.forward_job_view(endpoint, capture_id, job_id),
    )
    .await;
    match read {
        Ok(Ok(Some(job))) => encoded(view::forward_result_json(&job), "forward_result"),
        Ok(Ok(None)) => Outcome::Gone,
        Ok(Err(error)) => {
            tracing::warn!(%error, endpoint_id = %endpoint, %job_id, "cannot read the forward job for a result event");
            Outcome::Failed
        }
        Err(_) => {
            tracing::warn!(endpoint_id = %endpoint, %job_id, "reading the forward job for a result event timed out");
            Outcome::Failed
        }
    }
}
