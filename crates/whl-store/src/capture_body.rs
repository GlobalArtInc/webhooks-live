use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::capture_queries::CaptureError;

const BODY_SQL: &str = "SELECT b.data FROM captures c \
    JOIN capture_bodies b ON b.capture_id = c.id \
    WHERE c.endpoint_id = $1 AND c.id = $2";

impl Store {
    pub async fn capture_body(
        &self,
        endpoint: &EndpointId,
        capture: &CaptureId,
    ) -> Result<Option<Vec<u8>>, CaptureError> {
        sqlx::query_scalar::<_, Vec<u8>>(BODY_SQL)
            .bind(endpoint.unpack())
            .bind(capture.unpack())
            .fetch_optional(self.pool())
            .await
            .map_err(classify)
    }
}

fn classify(error: sqlx::Error) -> CaptureError {
    if matches!(
        error,
        sqlx::Error::PoolTimedOut | sqlx::Error::PoolClosed | sqlx::Error::Io(_)
    ) {
        CaptureError::Unavailable(error)
    } else {
        CaptureError::Query(error)
    }
}
