use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use sqlx::postgres::PgRow;
use sqlx::types::Uuid;
use whl_core::endpoint_id::EndpointId;

use crate::Store;
use crate::kind::CaptureKind;

pub(crate) const PATH_LIMIT_CHARS: i32 = 1024;
pub const NOTIFICATION_LIMIT_BYTES: usize = 7000;
pub const HEARTBEAT_NOTIFICATION: &str = r#"{"t":"heartbeat"}"#;

const QUERY_STRING: &str =
    "CASE WHEN strpos(c.url, '?') > 0 THEN substr(c.url, strpos(c.url, '?') + 1) ELSE '' END";

const FORWARD_STATE: &str = r#"(
    SELECT CASE
      WHEN bool_or(j.state = 'failed') THEN 'failed'
      WHEN bool_or(j.state IN ('pending', 'running')) THEN 'pending'
      ELSE 'succeeded'
    END
    FROM forward_jobs j
    WHERE j.capture_id = c.id
    HAVING count(*) > 0
  )"#;

fn timestamp(column: &str) -> String {
    format!(r#"to_char({column} AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')"#)
}

pub(crate) fn summary_columns(path_limit: Option<i32>) -> String {
    let (path, query_string) = match path_limit {
        Some(limit) => (
            format!("left(c.path, {limit})"),
            format!("left({QUERY_STRING}, {limit})"),
        ),
        None => ("c.path".to_owned(), QUERY_STRING.to_owned()),
    };
    let received_at = timestamp("c.received_at");
    format!(
        r#"
  c.id::text AS id,
  c.seq,
  c.endpoint_id,
  {received_at} AS received_at,
  c.method,
  {path} AS path,
  {query_string} AS query_string,
  host(c.client_ip) AS client_ip,
  c.content_type,
  c.body_size,
  c.status_override,
  c.response_status,
  {FORWARD_STATE} AS forward_state,
  c.kind"#
    )
}

fn endpoint_view_sql() -> String {
    let created_at = timestamp("e.created_at");
    let last_active_at = timestamp("e.last_active_at");
    let expires_at = timestamp(
        "(CASE WHEN e.never_expires THEN NULL \
         ELSE e.last_active_at + make_interval(secs => COALESCE(e.retention_seconds::float8, $2)) END)",
    );
    let rule_created_at = timestamp("r.created_at");
    format!(
        r#"
SELECT
  e.id,
  e.title,
  e.response_status,
  e.response_content_type,
  e.response_headers::text AS response_headers,
  e.response_body,
  e.response_delay_ms,
  e.cors,
  e.overflow,
  e.capture_count,
  e.bytes_used,
  e.last_seq,
  {created_at} AS created_at,
  {last_active_at} AS last_active_at,
  {expires_at} AS expires_at,
  e.retention_seconds,
  e.never_expires,
  e.max_captures,
  e.capture_ttl_seconds,
  (
    SELECT coalesce(json_agg(json_build_object(
      'id', r.id,
      'enabled', r.enabled,
      'target_url', r.target_url,
      'method', r.method,
      'content_type', r.content_type,
      'header_allowlist', r.header_allowlist,
      'append_path', r.append_path,
      'created_at', {rule_created_at}
    ) ORDER BY r.created_at, r.position), '[]'::json)
    FROM forward_rules r
    WHERE r.endpoint_id = e.id
  )::text AS forward_rules
FROM endpoints e
WHERE e.id = $1
  AND e.deleted_at IS NULL
  AND (e.never_expires OR e.last_active_at > now() - make_interval(secs => COALESCE(e.retention_seconds::float8, $2)))
"#
    )
}

fn forward_job_sql() -> String {
    let created_at = timestamp("j.created_at");
    let finished_at = timestamp("j.finished_at");
    format!(
        r#"
SELECT
  j.id::text AS id,
  j.rule_id::text AS rule_id,
  j.kind,
  j.target_url,
  j.method,
  j.state,
  {created_at} AS created_at,
  {finished_at} AS finished_at
FROM forward_jobs j
JOIN captures c ON c.id = j.capture_id
WHERE j.id = $1 AND j.capture_id = $2 AND c.endpoint_id = $3
"#
    )
}

fn forward_attempts_sql() -> String {
    let started_at = timestamp("started_at");
    format!(
        r#"
SELECT
  attempt,
  {started_at} AS started_at,
  duration_ms,
  status,
  error,
  final_url,
  response_content_type,
  response_body,
  response_truncated
FROM forward_attempts
WHERE job_id = $1
ORDER BY attempt ASC
"#
    )
}

const CAPTURES_AFTER: &str = r#"
WHERE c.endpoint_id = $1 AND c.seq > $2
ORDER BY c.seq ASC
LIMIT $3
"#;

const EXTEND_LIFETIME: &str = r#"
UPDATE endpoints
SET last_active_at = now()
WHERE id = $1
  AND deleted_at IS NULL
  AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2)))
RETURNING last_seq, capture_count
"#;

#[derive(Debug, thiserror::Error)]
pub enum StreamError {
    #[error("the stream query failed: {0}")]
    Query(#[from] sqlx::Error),
    #[error("a capture row carries the unknown forward state {state:?}")]
    UnknownForwardState { state: String },
    #[error("a capture row carries the unknown kind {kind:?}")]
    UnknownCaptureKind { kind: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ViewError {
    #[error("the view query failed: {0}")]
    Query(#[from] sqlx::Error),
    #[error("the stored forward rules of endpoint {endpoint} cannot be read: {source}")]
    ForwardRules {
        endpoint: String,
        source: serde_json::Error,
    },
    #[error("the stored response headers of endpoint {endpoint} cannot be read: {source}")]
    ResponseHeaders {
        endpoint: String,
        source: serde_json::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    #[error("the notification is {bytes} bytes, above the limit of {limit}")]
    TooLarge { bytes: usize, limit: usize },
    #[error("the notification cannot be encoded: {0}")]
    Encode(#[from] serde_json::Error),
}

pub fn encode_notification(value: &serde_json::Value) -> Result<String, NotificationError> {
    let payload = serde_json::to_string(value)?;
    if payload.len() > NOTIFICATION_LIMIT_BYTES {
        return Err(NotificationError::TooLarge {
            bytes: payload.len(),
            limit: NOTIFICATION_LIMIT_BYTES,
        });
    }
    Ok(payload)
}

pub fn endpoint_updated_notification(endpoint: &EndpointId) -> Result<String, NotificationError> {
    encode_notification(&json!({"t": "endpoint_updated", "e": endpoint.unpack()}))
}

pub fn forward_result_notification(
    endpoint: &EndpointId,
    capture_id: &Uuid,
    job_id: &Uuid,
) -> Result<String, NotificationError> {
    encode_notification(&json!({
        "t": "forward_result",
        "e": endpoint.unpack(),
        "d": {"capture_id": capture_id.to_string(), "job_id": job_id.to_string()},
    }))
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
    pub kind: CaptureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointLiveState {
    pub last_seq: i64,
    pub capture_count: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderView {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ForwardRuleView {
    pub id: String,
    pub enabled: bool,
    pub target_url: String,
    pub method: Option<String>,
    pub content_type: Option<String>,
    pub header_allowlist: Vec<String>,
    pub append_path: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointView {
    pub id: String,
    pub title: Option<String>,
    pub response_status: i16,
    pub response_content_type: String,
    pub response_headers: Vec<HeaderView>,
    pub response_body: Vec<u8>,
    pub response_delay_ms: i32,
    pub cors: bool,
    pub overflow: String,
    pub capture_count: i32,
    pub bytes_used: i64,
    pub last_seq: i64,
    pub created_at: String,
    pub last_active_at: String,
    pub expires_at: Option<String>,
    pub retention_seconds: Option<i32>,
    pub never_expires: bool,
    pub max_captures: Option<i32>,
    pub capture_ttl_seconds: Option<i32>,
    pub forward_rules: Vec<ForwardRuleView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardAttemptView {
    pub attempt: i16,
    pub started_at: String,
    pub duration_ms: i32,
    pub status: Option<i16>,
    pub error: Option<String>,
    pub final_url: Option<String>,
    pub response_content_type: Option<String>,
    pub response_body: Option<Vec<u8>>,
    pub response_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardJobView {
    pub id: String,
    pub capture_id: String,
    pub rule_id: Option<String>,
    pub kind: String,
    pub target_url: String,
    pub method: String,
    pub state: String,
    pub attempts: Vec<ForwardAttemptView>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

impl Store {
    pub async fn captures_after(
        &self,
        endpoint: &EndpointId,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<CaptureSummary>, StreamError> {
        let sql = format!(
            "SELECT {} FROM captures c {CAPTURES_AFTER}",
            summary_columns(Some(PATH_LIMIT_CHARS))
        );
        let rows = sqlx::query(&sql)
            .bind(endpoint.unpack())
            .bind(after_seq)
            .bind(limit)
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(summary_from_row).collect()
    }

    pub async fn endpoint_view(
        &self,
        endpoint: &EndpointId,
        retention: Duration,
    ) -> Result<Option<EndpointView>, ViewError> {
        let row = sqlx::query(&endpoint_view_sql())
            .bind(endpoint.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(endpoint_view_from_row).transpose()
    }

    pub async fn forward_job_view(
        &self,
        endpoint: &EndpointId,
        capture_id: &Uuid,
        job_id: &Uuid,
    ) -> Result<Option<ForwardJobView>, ViewError> {
        let Some(row) = sqlx::query(&forward_job_sql())
            .bind(job_id)
            .bind(capture_id)
            .bind(endpoint.unpack())
            .fetch_optional(self.pool())
            .await?
        else {
            return Ok(None);
        };
        let attempts = sqlx::query(&forward_attempts_sql())
            .bind(job_id)
            .fetch_all(self.pool())
            .await?;
        Ok(Some(ForwardJobView {
            id: row.try_get("id")?,
            capture_id: capture_id.to_string(),
            rule_id: row.try_get("rule_id")?,
            kind: row.try_get("kind")?,
            target_url: row.try_get("target_url")?,
            method: row.try_get("method")?,
            state: row.try_get("state")?,
            attempts: attempts
                .iter()
                .map(forward_attempt_from_row)
                .collect::<Result<_, _>>()?,
            created_at: row.try_get("created_at")?,
            finished_at: row.try_get("finished_at")?,
        }))
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

fn endpoint_view_from_row(row: &PgRow) -> Result<EndpointView, ViewError> {
    let id: String = row.try_get("id")?;
    let headers: String = row.try_get("response_headers")?;
    let rules: String = row.try_get("forward_rules")?;
    let response_headers =
        serde_json::from_str(&headers).map_err(|source| ViewError::ResponseHeaders {
            endpoint: id.clone(),
            source,
        })?;
    let forward_rules = serde_json::from_str(&rules).map_err(|source| ViewError::ForwardRules {
        endpoint: id.clone(),
        source,
    })?;
    Ok(EndpointView {
        id,
        title: row.try_get("title")?,
        response_status: row.try_get("response_status")?,
        response_content_type: row.try_get("response_content_type")?,
        response_headers,
        response_body: row.try_get("response_body")?,
        response_delay_ms: row.try_get("response_delay_ms")?,
        cors: row.try_get("cors")?,
        overflow: row.try_get("overflow")?,
        capture_count: row.try_get("capture_count")?,
        bytes_used: row.try_get("bytes_used")?,
        last_seq: row.try_get("last_seq")?,
        created_at: row.try_get("created_at")?,
        last_active_at: row.try_get("last_active_at")?,
        expires_at: row.try_get("expires_at")?,
        retention_seconds: row.try_get("retention_seconds")?,
        never_expires: row.try_get("never_expires")?,
        max_captures: row.try_get("max_captures")?,
        capture_ttl_seconds: row.try_get("capture_ttl_seconds")?,
        forward_rules,
    })
}

fn forward_attempt_from_row(row: &PgRow) -> Result<ForwardAttemptView, ViewError> {
    Ok(ForwardAttemptView {
        attempt: row.try_get("attempt")?,
        started_at: row.try_get("started_at")?,
        duration_ms: row.try_get("duration_ms")?,
        status: row.try_get("status")?,
        error: row.try_get("error")?,
        final_url: row.try_get("final_url")?,
        response_content_type: row.try_get("response_content_type")?,
        response_body: row.try_get("response_body")?,
        response_truncated: row.try_get("response_truncated")?,
    })
}

pub(crate) fn summary_from_row(row: &PgRow) -> Result<CaptureSummary, StreamError> {
    let forward_state: Option<String> = row.try_get("forward_state")?;
    let kind: String = row.try_get("kind")?;
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
        kind: CaptureKind::parse(&kind).ok_or(StreamError::UnknownCaptureKind { kind })?,
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
             'http://h/x/y?' || repeat('q', 2000), repeat('p', 2000), 'h', '10.1.2.3/32', '[]', '[]', 0, 0, 200 \
             FROM generate_series(1::bigint, $2::bigint) AS s",
        )
        .bind(id.unpack())
        .bind(count)
        .execute(db.pool())
        .await
        .expect("the captures insert");
        id
    }

    async fn add_job(db: &TestDb, endpoint: &EndpointId, seq: i64, state: &str) -> Uuid {
        sqlx::query_scalar(
            "INSERT INTO forward_jobs (id, capture_id, kind, target_url, method, header_allowlist, state) \
             SELECT gen_random_uuid(), id, 'manual', 'http://t', 'POST', '{}', $3 \
             FROM captures WHERE endpoint_id = $1 AND seq = $2 RETURNING id",
        )
        .bind(endpoint.unpack())
        .bind(seq)
        .bind(state)
        .fetch_one(db.pool())
        .await
        .expect("the job inserts")
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
        assert_eq!(first.query_string.chars().count(), 1024);
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
    async fn lifetime_extension_honours_expiry_and_deletion() {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let endpoint = endpoint_with_captures(&db, 2).await;
        sqlx::query("UPDATE endpoints SET capture_count = 2, last_seq = 2, last_active_at = now() - interval '10 minutes' WHERE id = $1")
            .bind(endpoint.unpack())
            .execute(db.pool())
            .await
            .expect("the endpoint updates");

        assert_eq!(
            store
                .extend_endpoint_lifetime(&endpoint, Duration::from_secs(60))
                .await
                .expect("the update succeeds"),
            None
        );
        assert_eq!(
            store
                .extend_endpoint_lifetime(&endpoint, RETENTION)
                .await
                .expect("the update succeeds"),
            Some(EndpointLiveState {
                last_seq: 2,
                capture_count: 2
            })
        );
        assert!(
            store
                .extend_endpoint_lifetime(&endpoint, Duration::from_secs(60))
                .await
                .expect("the update succeeds")
                .is_some()
        );

        sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
            .bind(endpoint.unpack())
            .execute(db.pool())
            .await
            .expect("the endpoint is marked deleted");
        assert_eq!(
            store
                .extend_endpoint_lifetime(&endpoint, RETENTION)
                .await
                .expect("the update succeeds"),
            None
        );
    }

    #[tokio::test]
    async fn reads_the_endpoint_view_with_its_rules_and_hides_dead_endpoints() {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let endpoint = endpoint_with_captures(&db, 1).await;
        sqlx::query(
            "UPDATE endpoints SET title = 'hook', cors = true, response_status = 202, \
             response_headers = '[{\"name\":\"X-A\",\"value\":\"1\"}]'::jsonb WHERE id = $1",
        )
        .bind(endpoint.unpack())
        .execute(db.pool())
        .await
        .expect("the endpoint updates");
        for (position, url) in [(1_i16, "http://zero"), (0, "http://one")] {
            sqlx::query(
                "INSERT INTO forward_rules (id, endpoint_id, position, target_url) \
                 VALUES (gen_random_uuid(), $1, $2, $3)",
            )
            .bind(endpoint.unpack())
            .bind(position)
            .bind(url)
            .execute(db.pool())
            .await
            .expect("the rule inserts");
        }

        let view = store
            .endpoint_view(&endpoint, RETENTION)
            .await
            .expect("the read succeeds")
            .expect("the endpoint is live");

        assert_eq!(view.id, endpoint.unpack());
        assert_eq!(view.title.as_deref(), Some("hook"));
        assert_eq!(view.response_status, 202);
        assert!(view.cors);
        assert_eq!(
            view.response_headers,
            vec![HeaderView {
                name: "X-A".to_owned(),
                value: "1".to_owned()
            }]
        );
        let targets: Vec<&str> = view
            .forward_rules
            .iter()
            .map(|rule| rule.target_url.as_str())
            .collect();
        assert_eq!(targets, vec!["http://zero", "http://one"]);
        assert!(view.forward_rules[0].created_at.ends_with('Z'));
        assert!(view.created_at.ends_with('Z'));
        assert!(view.expires_at.as_deref() > Some(view.last_active_at.as_str()));

        sqlx::query("UPDATE endpoints SET deleted_at = now() WHERE id = $1")
            .bind(endpoint.unpack())
            .execute(db.pool())
            .await
            .expect("the endpoint is marked deleted");
        assert_eq!(
            store
                .endpoint_view(&endpoint, RETENTION)
                .await
                .expect("the read succeeds"),
            None
        );
    }

    #[tokio::test]
    async fn reads_a_forward_job_with_its_attempts_only_for_the_owning_endpoint() {
        let db = TestDb::new().await;
        let store = Store::from_pool(db.pool().clone());
        let endpoint = endpoint_with_captures(&db, 1).await;
        let other = endpoint_with_captures(&db, 1).await;
        let job = add_job(&db, &endpoint, 1, "succeeded").await;
        let capture: Uuid = sqlx::query_scalar("SELECT capture_id FROM forward_jobs WHERE id = $1")
            .bind(job)
            .fetch_one(db.pool())
            .await
            .expect("the capture id is read");
        sqlx::query(
            "INSERT INTO forward_attempts (job_id, attempt, started_at, duration_ms, status, response_content_type, response_body, response_truncated) \
             VALUES ($1, 1, now(), 12, 200, 'text/plain', 'ok'::bytea, true), ($1, 2, now(), 7, NULL, NULL, NULL, false)",
        )
        .bind(job)
        .execute(db.pool())
        .await
        .expect("the attempts insert");

        let view = store
            .forward_job_view(&endpoint, &capture, &job)
            .await
            .expect("the read succeeds")
            .expect("the job exists");

        assert_eq!(view.id, job.to_string());
        assert_eq!(view.capture_id, capture.to_string());
        assert_eq!(view.state, "succeeded");
        assert_eq!(view.attempts.len(), 2);
        assert_eq!(view.attempts[0].status, Some(200));
        assert_eq!(view.attempts[0].response_body.as_deref(), Some(&b"ok"[..]));
        assert_eq!(
            view.attempts[0].response_content_type.as_deref(),
            Some("text/plain")
        );
        assert!(view.attempts[0].response_truncated);
        assert_eq!(view.attempts[1].response_body, None);
        assert!(!view.attempts[1].response_truncated);
        assert_eq!(view.attempts[1].status, None);
        assert_eq!(
            store
                .forward_job_view(&other, &capture, &job)
                .await
                .expect("the read succeeds"),
            None
        );
    }

    #[test]
    fn the_notification_encoder_builds_thin_payloads_and_refuses_oversize_ones() {
        let endpoint = EndpointId::generate().expect("the operating system provides entropy");
        let capture = Uuid::nil();
        let job = Uuid::max();

        let updated = endpoint_updated_notification(&endpoint).expect("a thin payload encodes");
        assert_eq!(
            updated,
            format!(r#"{{"e":"{}","t":"endpoint_updated"}}"#, endpoint.unpack())
        );
        let result =
            forward_result_notification(&endpoint, &capture, &job).expect("a thin payload encodes");
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("the payload is JSON");
        assert_eq!(parsed["d"]["job_id"], job.to_string());
        assert_eq!(parsed["d"]["capture_id"], capture.to_string());

        let at_limit = "x".repeat(NOTIFICATION_LIMIT_BYTES - r#"{"p":""}"#.len());
        assert_eq!(
            encode_notification(&json!({"p": at_limit}))
                .expect("the limit itself is allowed")
                .len(),
            NOTIFICATION_LIMIT_BYTES
        );
        let over = "x".repeat(NOTIFICATION_LIMIT_BYTES - r#"{"p":""}"#.len() + 1);
        assert!(matches!(
            encode_notification(&json!({"p": over})),
            Err(NotificationError::TooLarge { .. })
        ));
    }
}
