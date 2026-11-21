use std::time::Duration;

use sqlx::Row;
use sqlx::postgres::PgRow;
use whl_core::endpoint_id::EndpointId;

use crate::Store;

const PATH_LIMIT_CHARS: i32 = 1024;

const CAPTURES_AFTER: &str = r#"
SELECT
  c.id::text AS id,
  c.seq,
  c.endpoint_id,
  to_char(c.received_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') AS received_at,
  c.method,
  left(c.path, $4) AS path,
  CASE WHEN strpos(c.url, '?') > 0 THEN substr(c.url, strpos(c.url, '?') + 1) ELSE '' END AS query_string,
  host(c.client_ip) AS client_ip,
  c.content_type,
  c.body_size,
  c.status_override,
  c.response_status,
  (
    SELECT CASE
      WHEN bool_or(j.state = 'failed') THEN 'failed'
      WHEN bool_or(j.state IN ('pending', 'running')) THEN 'pending'
      ELSE 'succeeded'
    END
    FROM forward_jobs j
    WHERE j.capture_id = c.id
    HAVING count(*) > 0
  ) AS forward_state
FROM captures c
WHERE c.endpoint_id = $1 AND c.seq > $2
ORDER BY c.seq ASC
LIMIT $3
"#;

const EXTEND_LIFETIME: &str = r#"
UPDATE endpoints
SET last_active_at = now()
WHERE id = $1
  AND deleted_at IS NULL
  AND last_active_at > now() - make_interval(secs => $2)
RETURNING last_seq, capture_count
"#;

const LIVE_STATE: &str = r#"
SELECT last_seq, capture_count
FROM endpoints
WHERE id = $1
  AND deleted_at IS NULL
  AND last_active_at > now() - make_interval(secs => $2)
"#;

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("the stream query failed: {0}")]
    Query(#[from] sqlx::Error),
    #[error("a capture row carries the unknown forward state {state:?}")]
    UnknownForwardState { state: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardState {
    Pending,
    Succeeded,
    Failed,
}

impl ForwardState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn parse(text: &str) -> Result<Self, StreamError> {
        match text {
            "pending" => Ok(Self::Pending),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            other => Err(StreamError::UnknownForwardState {
                state: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureSummary {
    pub id: String,
    pub seq: i64,
    pub endpoint_id: String,
    pub received_at: String,
    pub method: String,
    pub path: String,
    pub query_string: String,
    pub client_ip: String,
    pub content_type: Option<String>,
    pub body_size: i32,
    pub status_override: Option<i16>,
    pub response_status: i16,
    pub forward_state: Option<ForwardState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointLiveState {
    pub last_seq: i64,
    pub capture_count: i32,
}

impl Store {
    pub async fn captures_after(
        &self,
        endpoint: &EndpointId,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<CaptureSummary>, StreamError> {
        let rows = sqlx::query(CAPTURES_AFTER)
            .bind(endpoint.unpack())
            .bind(after_seq)
            .bind(limit)
            .bind(PATH_LIMIT_CHARS)
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(summary_from_row).collect()
    }

    pub async fn endpoint_live_state(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
    ) -> Result<Option<EndpointLiveState>, StreamError> {
        let row = sqlx::query(LIVE_STATE)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(live_state_from_row).transpose()
    }

    pub async fn extend_endpoint_lifetime(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
    ) -> Result<Option<EndpointLiveState>, StreamError> {
        let row = sqlx::query(EXTEND_LIFETIME)
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(live_state_from_row).transpose()
    }
}

fn live_state_from_row(row: &PgRow) -> Result<EndpointLiveState, StreamError> {
    Ok(EndpointLiveState {
        last_seq: row.try_get("last_seq")?,
        capture_count: row.try_get("capture_count")?,
    })
}

fn summary_from_row(row: &PgRow) -> Result<CaptureSummary, StreamError> {
    let forward_state: Option<String> = row.try_get("forward_state")?;
    Ok(CaptureSummary {
        id: row.try_get("id")?,
        seq: row.try_get("seq")?,
        endpoint_id: row.try_get("endpoint_id")?,
        received_at: row.try_get("received_at")?,
        method: row.try_get("method")?,
        path: row.try_get("path")?,
        query_string: row.try_get("query_string")?,
        client_ip: row.try_get("client_ip")?,
        content_type: row.try_get("content_type")?,
        body_size: row.try_get("body_size")?,
        status_override: row.try_get("status_override")?,
        response_status: row.try_get("response_status")?,
        forward_state: forward_state
            .as_deref()
            .map(ForwardState::parse)
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use whl_testkit::TestDb;

    use super::*;

    const RETENTION: Duration = Duration::from_secs(3600);

    async fn endpoint_with_captures(db: &TestDb, count: i64) -> EndpointId {
        let id = EndpointId::generate().expect("the operating system provides entropy");
        sqlx::query("INSERT INTO endpoints (id) VALUES ($1)")
            .bind(id.unpack())
            .execute(db.pool())
            .await
            .expect("the endpoint inserts");
        sqlx::query(
            "INSERT INTO captures (endpoint_id, seq, id, received_at, method, url, path, host, \
             client_ip, headers, query, body_size, stored_bytes, response_status) \
             SELECT $1, s, gen_random_uuid(), '2026-11-21T10:15:30.123456Z', 'GET', \
             'http://h/x/y?q=1&r=2', repeat('p', 2000), 'h', '10.1.2.3/32', '[]', '[]', 0, 0, 200 \
             FROM generate_series(1::bigint, $2::bigint) AS s",
        )
        .bind(id.unpack())
        .bind(count)
        .execute(db.pool())
        .await
        .expect("the captures insert");
        id
    }

    async fn add_job(db: &TestDb, endpoint: &EndpointId, seq: i64, state: &str) {
        sqlx::query(
            "INSERT INTO forward_jobs (id, capture_id, kind, target_url, method, header_allowlist, state) \
             SELECT gen_random_uuid(), id, 'manual', 'http://t', 'POST', '{}', $3 \
             FROM captures WHERE endpoint_id = $1 AND seq = $2",
        )
        .bind(endpoint.unpack())
        .bind(seq)
        .bind(state)
        .execute(db.pool())
        .await
        .expect("the job inserts");
    }

    #[tokio::test]
    async fn reads_captures_after_a_seq_ascending_within_the_limit() {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let endpoint = endpoint_with_captures(&db, 10).await;

        let rows = store
            .captures_after(&endpoint, 3, 4)
            .await
            .expect("the read succeeds");

        let seqs: Vec<i64> = rows.iter().map(|row| row.seq).collect();
        assert_eq!(seqs, vec![4, 5, 6, 7]);
        let first = &rows[0];
        assert_eq!(first.received_at, "2026-11-21T10:15:30.123Z");
        assert_eq!(first.client_ip, "10.1.2.3");
        assert_eq!(first.query_string, "q=1&r=2");
        assert_eq!(first.path.chars().count(), 1024);
        assert_eq!(first.content_type, None);
        assert_eq!(first.forward_state, None);
        assert_eq!(first.endpoint_id, endpoint.unpack());
    }

    #[tokio::test]
    async fn aggregates_the_worst_forward_state_per_capture() {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let endpoint = endpoint_with_captures(&db, 4).await;
        add_job(&db, &endpoint, 1, "succeeded").await;
        add_job(&db, &endpoint, 2, "succeeded").await;
        add_job(&db, &endpoint, 2, "running").await;
        add_job(&db, &endpoint, 3, "pending").await;
        add_job(&db, &endpoint, 3, "failed").await;

        let rows = store
            .captures_after(&endpoint, 0, 10)
            .await
            .expect("the read succeeds");

        let states: Vec<Option<ForwardState>> = rows.iter().map(|row| row.forward_state).collect();
        assert_eq!(
            states,
            vec![
                Some(ForwardState::Succeeded),
                Some(ForwardState::Pending),
                Some(ForwardState::Failed),
                None
            ]
        );
    }

    #[tokio::test]
    async fn live_state_and_lifetime_extension_honour_expiry_and_deletion() {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let endpoint = endpoint_with_captures(&db, 2).await;
        sqlx::query("UPDATE endpoints SET capture_count = 2, last_seq = 2, last_active_at = now() - interval '10 minutes' WHERE id = $1")
            .bind(endpoint.unpack())
            .execute(db.pool())
            .await
            .expect("the endpoint updates");

        let state = store
            .endpoint_live_state(&endpoint, RETENTION)
            .await
            .expect("the read succeeds");
        assert_eq!(
            state,
            Some(EndpointLiveState {
                last_seq: 2,
                capture_count: 2
            })
        );
        assert_eq!(
            store
                .endpoint_live_state(&endpoint, Duration::from_secs(60))
                .await
                .expect("the read succeeds"),
            None
        );
        assert_eq!(
            store
                .extend_endpoint_lifetime(&endpoint, Duration::from_secs(60))
                .await
                .expect("the update succeeds"),
            None
        );

        assert!(
            store
                .extend_endpoint_lifetime(&endpoint, RETENTION)
                .await
                .expect("the update succeeds")
                .is_some()
        );
        assert!(
            store
                .endpoint_live_state(&endpoint, Duration::from_secs(60))
                .await
                .expect("the read succeeds")
                .is_some()
        );

        sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
            .bind(endpoint.unpack())
            .execute(db.pool())
            .await
            .expect("the endpoint is marked deleted");
        assert_eq!(
            store
                .endpoint_live_state(&endpoint, RETENTION)
                .await
                .expect("the read succeeds"),
            None
        );
    }
}
