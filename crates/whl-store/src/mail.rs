use ipnet::IpNet;
use sqlx::Row;
use whl_core::endpoint_id::EndpointId;

use crate::captures::{CapturePolicy, CaptureStoreError, TRANSACTION_DEADLINE};
use crate::store::Store;

const MAIL_TARGET: &str = "SELECT (capture_count >= $3 AND overflow <> 'ring') AS full, \
       sender_allow_cidrs::text[] AS allow_cidrs \
     FROM endpoints \
     WHERE id = $1 AND deleted_at IS NULL AND email_enabled \
     AND last_active_at > now() - make_interval(secs => $2)";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailTarget {
    pub allow_cidrs: Vec<IpNet>,
    pub full: bool,
}

impl Store {
    pub async fn mail_target(
        &self,
        id: &EndpointId,
        policy: CapturePolicy,
    ) -> Result<Option<MailTarget>, CaptureStoreError> {
        let mut connection = self.acquire_capture_connection().await?;
        let query = sqlx::query(MAIL_TARGET)
            .bind(id.unpack())
            .bind(policy.retention.as_secs_f64())
            .bind(i64::from(policy.max_captures))
            .fetch_optional(&mut *connection);
        let row = tokio::time::timeout(TRANSACTION_DEADLINE, query)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
            .map_err(CaptureStoreError::classify)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let full: bool = row.try_get("full").map_err(CaptureStoreError::classify)?;
        let texts: Vec<String> = row
            .try_get("allow_cidrs")
            .map_err(CaptureStoreError::classify)?;
        let allow_cidrs = texts
            .iter()
            .map(|text| {
                text.parse::<IpNet>().map_err(|error| {
                    CaptureStoreError::Corrupt(format!("sender allow list entry {text:?}: {error}"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(MailTarget { allow_cidrs, full }))
    }
}
