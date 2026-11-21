use std::net::IpAddr;
use std::time::Duration;

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sqlx::pool::PoolConnection;
use sqlx::{Postgres, Row, Transaction};
use time::OffsetDateTime;
use time::macros::format_description;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::status::HttpStatus;
use whl_secrets::{KeyId, SealedSecret};
use whl_verify::{Verification, Verifier, VerifierDocument};

use crate::kind::CaptureKind;
use crate::store::Store;

pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(1);
pub const TRANSACTION_DEADLINE: Duration = Duration::from_secs(6);
pub const EVENT_CHANNEL: &str = "whl_events";
pub const PATH_SUMMARY_BYTES: usize = 1024;
pub const SEARCH_BODY_BYTES: usize = 256 * 1024;

pub(crate) const EVENT_PAYLOAD_LIMIT: usize = 7900;
pub(crate) const MAX_EVICTIONS_PER_CAPTURE: i64 = 50;
const NUL: char = '\0';

pub(crate) const SESSION_LIMITS: &str = "SELECT \
       set_config('statement_timeout', '2s', true), \
       set_config('idle_in_transaction_session_timeout', '5s', true)";

const ENDPOINT_LIVE: &str = "SELECT 1 FROM endpoints \
     WHERE id = $1 AND deleted_at IS NULL \
     AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2)))";

const ENDPOINT_ADMISSION: &str = "SELECT (e.capture_count >= COALESCE(e.max_captures::bigint, $3) AND e.overflow <> 'ring') AS full, \
       e.verifier::text AS verifier, s.key_id, s.nonce, s.ciphertext \
     FROM endpoints e \
     LEFT JOIN endpoint_secrets s ON e.verifier IS NOT NULL \
       AND s.endpoint_id = e.id AND s.name = e.verifier ->> 'secret_name' \
     WHERE e.id = $1 AND e.deleted_at IS NULL \
     AND (e.never_expires OR e.last_active_at > now() - make_interval(secs => COALESCE(e.retention_seconds::float8, $2)))";

const CLAIM_SLOT: &str = "UPDATE endpoints SET \
       capture_count = capture_count + 1, \
       bytes_used = bytes_used + $2, \
       last_seq = last_seq + 1, \
       last_active_at = GREATEST(last_active_at, now()) \
     WHERE id = $1 AND deleted_at IS NULL \
       AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $3))) \
       AND (capture_count < COALESCE(max_captures::bigint, $4) OR (overflow = 'ring' AND EXISTS ( \
         SELECT 1 FROM captures WHERE endpoint_id = $1 AND NOT starred))) \
     RETURNING last_seq, capture_count, COALESCE(max_captures::bigint, $4) AS effective_max, response_status, response_content_type, \
       response_headers::text AS response_headers, response_body, response_delay_ms, cors, \
       title, response_templated, response_status_template";

pub(crate) const EVICT_OLDEST: &str = "WITH evicted AS ( \
       DELETE FROM captures WHERE endpoint_id = $1 AND seq IN ( \
         SELECT seq FROM captures WHERE endpoint_id = $1 AND NOT starred ORDER BY seq LIMIT $2) \
       RETURNING stored_bytes) \
     UPDATE endpoints SET \
       capture_count = capture_count - (SELECT count(*) FROM evicted), \
       bytes_used = bytes_used - (SELECT coalesce(sum(stored_bytes), 0) FROM evicted) \
     WHERE id = $1";

const INSERT_CAPTURE: &str = "INSERT INTO captures \
     (endpoint_id, seq, id, received_at, method, url, path, host, client_ip, headers, query, \
      form, content_type, body_size, stored_bytes, status_override, response_status, kind, meta, \
      verification) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text::inet, $10::text::jsonb, $11::text::jsonb, \
      $12::text::jsonb, $13, $14, $15, $16, $17, $18, $19::text::jsonb, $20)";

pub(crate) const INSERT_BODY: &str =
    "INSERT INTO capture_bodies (capture_id, data, search_text) VALUES ($1, $2, $3)";

pub(crate) const NOTIFY: &str = "SELECT pg_notify($1, $2)";

const RECORD_RENDER: &str = "UPDATE captures SET \
       meta = COALESCE(meta, '{}'::jsonb) || $2::text::jsonb, \
       response_status = COALESCE($3, response_status) \
     WHERE id = $1";

const RENDER_RECORD_DEADLINE: Duration = Duration::from_millis(500);

#[derive(Debug, thiserror::Error)]
pub enum CaptureStoreError {
    #[error("the database is unavailable: {source}")]
    Unavailable {
        #[source]
        source: sqlx::Error,
    },
    #[error("no database connection became free within {ACQUIRE_TIMEOUT:?}")]
    AcquireTimeout,
    #[error("the capture transaction did not finish within {TRANSACTION_DEADLINE:?}")]
    DeadlineExceeded,
    #[error("the capture query failed: {source}")]
    Query {
        #[source]
        source: sqlx::Error,
    },
    #[error("stored endpoint data is malformed: {0}")]
    Corrupt(String),
    #[error("cannot encode capture data: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("{0} is out of the supported range")]
    OutOfRange(&'static str),
    #[error("the capture notification does not fit the {EVENT_PAYLOAD_LIMIT} byte limit")]
    PayloadTooLarge,
    #[error("the capture timestamp cannot be formatted: {0}")]
    Timestamp(#[from] time::error::Format),
}

impl CaptureStoreError {
    pub fn is_unavailable(&self) -> bool {
        matches!(
            self,
            Self::Unavailable { .. } | Self::AcquireTimeout | Self::DeadlineExceeded
        )
    }

    pub(crate) fn classify(source: sqlx::Error) -> Self {
        let unavailable = match &source {
            sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::Protocol(_)
            | sqlx::Error::WorkerCrashed => true,
            sqlx::Error::Database(database) => database.code().is_some_and(|code| {
                ["08", "40", "53", "55", "57"]
                    .iter()
                    .any(|class| code.starts_with(class))
            }),
            _ => false,
        };
        if unavailable {
            Self::Unavailable { source }
        } else {
            Self::Query { source }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormFile {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Form {
    pub fields: Vec<NameValue>,
    pub files: Vec<FormFile>,
}

#[derive(Debug, Clone)]
pub struct NewCapture {
    pub id: CaptureId,
    pub endpoint_id: EndpointId,
    pub received_at: OffsetDateTime,
    pub method: String,
    pub url: String,
    pub path: String,
    pub query_string: String,
    pub host: String,
    pub client_ip: IpAddr,
    pub headers: Vec<NameValue>,
    pub query: Vec<NameValue>,
    pub form: Option<Form>,
    pub content_type: Option<String>,
    pub body: Bytes,
    pub status_override: Option<HttpStatus>,
}

#[derive(Debug, Clone)]
pub struct CaptureShape {
    pub kind: CaptureKind,
    pub meta: serde_json::Value,
    pub response_status: Option<u16>,
    pub verification: Option<Verification>,
}

impl Default for CaptureShape {
    fn default() -> Self {
        Self {
            kind: CaptureKind::Http,
            meta: serde_json::json!({}),
            response_status: None,
            verification: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturePolicy {
    pub max_captures: u32,
    pub retention: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointState {
    Live,
    Full,
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifierSource {
    Ready {
        verifier: Verifier,
        sealed: Option<SealedSecret>,
    },
    Unusable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointAdmission {
    pub state: EndpointState,
    pub verifier: Option<VerifierSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub id: EndpointId,
    pub status: HttpStatus,
    pub content_type: String,
    pub headers: Vec<NameValue>,
    pub body: Vec<u8>,
    pub delay_ms: u32,
    pub cors: bool,
    pub title: Option<String>,
    pub templated: bool,
    pub status_template: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RenderRecord {
    pub meta: serde_json::Value,
    pub response_status: Option<HttpStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCapture {
    pub id: CaptureId,
    pub seq: i64,
    pub received_at: OffsetDateTime,
    pub response_status: HttpStatus,
    pub body_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureOutcome {
    Stored {
        endpoint: Box<Endpoint>,
        capture: StoredCapture,
    },
    Gone,
    Full,
}

impl NewCapture {
    fn scrub(&mut self) {
        scrub_string(&mut self.method);
        scrub_string(&mut self.url);
        scrub_string(&mut self.path);
        scrub_string(&mut self.query_string);
        scrub_string(&mut self.host);
        scrub_pairs(&mut self.headers);
        scrub_pairs(&mut self.query);
        if let Some(content_type) = &mut self.content_type {
            scrub_string(content_type);
        }
        if let Some(form) = &mut self.form {
            scrub_pairs(&mut form.fields);
            for file in &mut form.files {
                scrub_string(&mut file.name);
                if let Some(filename) = &mut file.filename {
                    scrub_string(filename);
                }
                if let Some(content_type) = &mut file.content_type {
                    scrub_string(content_type);
                }
            }
        }
    }

    fn search_text(&self) -> String {
        let prefix = &self.body[..self.body.len().min(SEARCH_BODY_BYTES)];
        let mut text = String::with_capacity(self.path.len() + self.query_string.len() + 64);
        text.push_str(&self.path);
        text.push('\n');
        text.push_str(&self.query_string);
        text.push('\n');
        for header in &self.headers {
            text.push_str(&header.name);
            text.push_str(": ");
            text.push_str(&header.value);
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(prefix));
        let mut lowered = text.to_lowercase();
        scrub_string(&mut lowered);
        lowered
    }
}

pub(crate) fn scrub_string(text: &mut String) {
    if text.contains(NUL) {
        *text = text.replace(NUL, "\u{FFFD}");
    }
}

fn scrub_pairs(pairs: &mut [NameValue]) {
    for pair in pairs {
        scrub_string(&mut pair.name);
        scrub_string(&mut pair.value);
    }
}

pub(crate) fn truncate(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[derive(Serialize)]
struct Summary<'a> {
    id: String,
    seq: i64,
    endpoint_id: &'a str,
    received_at: String,
    method: &'a str,
    path: &'a str,
    query_string: &'a str,
    client_ip: String,
    content_type: Option<&'a str>,
    body_size: usize,
    status_override: Option<u16>,
    response_status: u16,
    forward_state: Option<&'static str>,
    kind: &'static str,
}

#[derive(Serialize)]
struct Event<'a> {
    t: &'static str,
    e: &'a str,
    seq: i64,
    s: Summary<'a>,
}

#[derive(Clone, Copy)]
struct SummaryCaps {
    path: usize,
    query_string: usize,
    content_type: usize,
    method: usize,
}

const SUMMARY_TIERS: [SummaryCaps; 3] = [
    SummaryCaps {
        path: PATH_SUMMARY_BYTES,
        query_string: PATH_SUMMARY_BYTES,
        content_type: 255,
        method: 32,
    },
    SummaryCaps {
        path: 256,
        query_string: 256,
        content_type: 64,
        method: 16,
    },
    SummaryCaps {
        path: 0,
        query_string: 0,
        content_type: 0,
        method: 0,
    },
];

pub fn capture_event_payload(
    capture: &NewCapture,
    seq: i64,
    response_status: HttpStatus,
) -> Result<String, CaptureStoreError> {
    event_payload(capture, seq, response_status.unpack(), CaptureKind::Http)
}

fn event_payload(
    capture: &NewCapture,
    seq: i64,
    response_status: u16,
    kind: CaptureKind,
) -> Result<String, CaptureStoreError> {
    let received_at = format_timestamp(capture.received_at)?;
    for caps in SUMMARY_TIERS {
        let summary = Summary {
            id: capture.id.to_string(),
            seq,
            endpoint_id: capture.endpoint_id.unpack(),
            received_at: received_at.clone(),
            method: truncate(&capture.method, caps.method),
            path: truncate(&capture.path, caps.path),
            query_string: truncate(&capture.query_string, caps.query_string),
            client_ip: capture.client_ip.to_string(),
            content_type: capture
                .content_type
                .as_deref()
                .map(|value| truncate(value, caps.content_type)),
            body_size: capture.body.len(),
            status_override: capture.status_override.map(|status| status.unpack()),
            response_status,
            forward_state: None,
            kind: kind.as_str(),
        };
        let payload = serde_json::to_string(&Event {
            t: "capture",
            e: capture.endpoint_id.unpack(),
            seq,
            s: summary,
        })?;
        if payload.len() <= EVENT_PAYLOAD_LIMIT {
            return Ok(payload);
        }
    }
    Err(CaptureStoreError::PayloadTooLarge)
}

pub(crate) fn format_timestamp(moment: OffsetDateTime) -> Result<String, CaptureStoreError> {
    let utc = moment.to_offset(time::UtcOffset::UTC);
    Ok(utc.format(format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
    ))?)
}

pub(crate) fn to_i32(value: usize, what: &'static str) -> Result<i32, CaptureStoreError> {
    i32::try_from(value).map_err(|_| CaptureStoreError::OutOfRange(what))
}

impl Store {
    pub async fn endpoint_state(
        &self,
        id: &EndpointId,
        policy: CapturePolicy,
    ) -> Result<EndpointState, CaptureStoreError> {
        Ok(self.endpoint_admission(id, policy).await?.state)
    }

    pub async fn endpoint_admission(
        &self,
        id: &EndpointId,
        policy: CapturePolicy,
    ) -> Result<EndpointAdmission, CaptureStoreError> {
        let mut connection = self.acquire_capture_connection().await?;
        let query = sqlx::query(ENDPOINT_ADMISSION)
            .bind(id.unpack())
            .bind(policy.retention.as_secs_f64())
            .bind(i64::from(policy.max_captures))
            .fetch_optional(&mut *connection);
        let row = tokio::time::timeout(TRANSACTION_DEADLINE, query)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
            .map_err(CaptureStoreError::classify)?;
        let Some(row) = row else {
            return Ok(EndpointAdmission {
                state: EndpointState::Gone,
                verifier: None,
            });
        };
        let full: bool = row.try_get("full").map_err(CaptureStoreError::classify)?;
        Ok(EndpointAdmission {
            state: if full {
                EndpointState::Full
            } else {
                EndpointState::Live
            },
            verifier: verifier_from_row(&row)?,
        })
    }

    pub async fn store_capture(
        &self,
        capture: NewCapture,
        policy: CapturePolicy,
    ) -> Result<CaptureOutcome, CaptureStoreError> {
        self.store_capture_as(capture, policy, CaptureShape::default())
            .await
    }

    pub async fn store_verified_capture(
        &self,
        capture: NewCapture,
        verification: Option<Verification>,
        policy: CapturePolicy,
    ) -> Result<CaptureOutcome, CaptureStoreError> {
        self.store_capture_as(
            capture,
            policy,
            CaptureShape {
                verification,
                ..CaptureShape::default()
            },
        )
        .await
    }

    pub async fn store_capture_as(
        &self,
        mut capture: NewCapture,
        policy: CapturePolicy,
        shape: CaptureShape,
    ) -> Result<CaptureOutcome, CaptureStoreError> {
        self.store_capture_retained_as(&mut capture, policy, shape)
            .await
    }

    pub async fn store_capture_retained(
        &self,
        capture: &mut NewCapture,
        verification: Option<Verification>,
        policy: CapturePolicy,
    ) -> Result<CaptureOutcome, CaptureStoreError> {
        self.store_capture_retained_as(
            capture,
            policy,
            CaptureShape {
                verification,
                ..CaptureShape::default()
            },
        )
        .await
    }

    async fn store_capture_retained_as(
        &self,
        capture: &mut NewCapture,
        policy: CapturePolicy,
        shape: CaptureShape,
    ) -> Result<CaptureOutcome, CaptureStoreError> {
        capture.scrub();
        let meta = shape.meta.to_string();
        let prepared = Prepared::new(capture, meta)?;
        let tx = self.begin_capture_transaction().await?;
        let work = execute_capture(tx, capture, prepared, policy, shape);
        tokio::time::timeout(TRANSACTION_DEADLINE, work)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
    }

    pub async fn record_render(
        &self,
        capture: &CaptureId,
        update: &RenderRecord,
    ) -> Result<(), CaptureStoreError> {
        let meta = serde_json::to_string(&update.meta)?;
        let status = update
            .response_status
            .map(|status| i16::try_from(status.unpack()))
            .transpose()
            .map_err(|_| CaptureStoreError::OutOfRange("response status"))?;
        let mut connection = self.acquire_capture_connection().await?;
        let query = sqlx::query(RECORD_RENDER)
            .bind(capture.unpack())
            .bind(meta)
            .bind(status)
            .execute(&mut *connection);
        tokio::time::timeout(RENDER_RECORD_DEADLINE, query)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
            .map_err(CaptureStoreError::classify)?;
        Ok(())
    }

    pub(crate) async fn acquire_capture_connection(
        &self,
    ) -> Result<PoolConnection<Postgres>, CaptureStoreError> {
        tokio::time::timeout(ACQUIRE_TIMEOUT, self.pool().acquire())
            .await
            .map_err(|_| CaptureStoreError::AcquireTimeout)?
            .map_err(CaptureStoreError::classify)
    }

    pub(crate) async fn begin_capture_transaction(
        &self,
    ) -> Result<Transaction<'static, Postgres>, CaptureStoreError> {
        tokio::time::timeout(ACQUIRE_TIMEOUT, self.pool().begin())
            .await
            .map_err(|_| CaptureStoreError::AcquireTimeout)?
            .map_err(CaptureStoreError::classify)
    }
}

struct Prepared {
    headers: String,
    query: String,
    form: Option<String>,
    meta: String,
    search_text: String,
    body_size: i32,
    stored_bytes: i32,
}

impl Prepared {
    fn new(capture: &NewCapture, meta: String) -> Result<Self, CaptureStoreError> {
        let headers = serde_json::to_string(&capture.headers)?;
        let query = serde_json::to_string(&capture.query)?;
        let form = capture
            .form
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let search_text = capture.search_text();
        let stored = capture.body.len()
            + headers.len()
            + query.len()
            + form.as_ref().map_or(0, String::len)
            + meta.len()
            + search_text.len();
        Ok(Self {
            body_size: to_i32(capture.body.len(), "body size")?,
            stored_bytes: to_i32(stored, "stored bytes")?,
            headers,
            query,
            form,
            meta,
            search_text,
        })
    }
}

macro_rules! attempt {
    ($tx:ident, $step:expr) => {
        match $step {
            Ok(value) => value,
            Err(error) => return abort($tx, error).await,
        }
    };
}

async fn abort(
    tx: Transaction<'static, Postgres>,
    error: CaptureStoreError,
) -> Result<CaptureOutcome, CaptureStoreError> {
    if let Err(rollback) = tx.rollback().await {
        tracing::warn!(%rollback, "rollback after a failed capture also failed");
    }
    Err(error)
}

async fn execute_capture(
    mut tx: Transaction<'static, Postgres>,
    capture: &NewCapture,
    prepared: Prepared,
    policy: CapturePolicy,
    shape: CaptureShape,
) -> Result<CaptureOutcome, CaptureStoreError> {
    let max_captures = i64::from(policy.max_captures);
    attempt!(
        tx,
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)
    );

    let claimed = attempt!(
        tx,
        sqlx::query(CLAIM_SLOT)
            .bind(capture.endpoint_id.unpack())
            .bind(i64::from(prepared.stored_bytes))
            .bind(policy.retention.as_secs_f64())
            .bind(max_captures)
            .fetch_optional(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)
    );
    let Some(row) = claimed else {
        let live = attempt!(
            tx,
            sqlx::query(ENDPOINT_LIVE)
                .bind(capture.endpoint_id.unpack())
                .bind(policy.retention.as_secs_f64())
                .fetch_optional(&mut *tx)
                .await
                .map_err(CaptureStoreError::classify)
        );
        let outcome = if live.is_some() {
            CaptureOutcome::Full
        } else {
            CaptureOutcome::Gone
        };
        tx.rollback().await.map_err(CaptureStoreError::classify)?;
        return Ok(outcome);
    };

    let seq: i64 = attempt!(
        tx,
        row.try_get("last_seq").map_err(CaptureStoreError::classify)
    );
    let count_after: i32 = attempt!(
        tx,
        row.try_get("capture_count")
            .map_err(CaptureStoreError::classify)
    );
    let effective_max: i64 = attempt!(
        tx,
        row.try_get("effective_max")
            .map_err(CaptureStoreError::classify)
    );
    let endpoint = attempt!(tx, endpoint_from_row(&row, &capture.endpoint_id));

    let surplus = i64::from(count_after) - effective_max;
    if surplus > 0 {
        attempt!(
            tx,
            sqlx::query(EVICT_OLDEST)
                .bind(capture.endpoint_id.unpack())
                .bind(surplus.min(MAX_EVICTIONS_PER_CAPTURE))
                .execute(&mut *tx)
                .await
                .map_err(CaptureStoreError::classify)
        );
    }

    let response_status = capture.status_override.unwrap_or(endpoint.status);
    let id = capture.id.unpack();
    let override_status = attempt!(
        tx,
        capture
            .status_override
            .map(|status| i16::try_from(status.unpack()))
            .transpose()
            .map_err(|_| CaptureStoreError::OutOfRange("status override"))
    );
    let stored_status = shape
        .response_status
        .unwrap_or_else(|| response_status.unpack());
    let final_status = attempt!(
        tx,
        i16::try_from(stored_status).map_err(|_| CaptureStoreError::OutOfRange("response status"))
    );
    attempt!(
        tx,
        sqlx::query(INSERT_CAPTURE)
            .bind(capture.endpoint_id.unpack())
            .bind(seq)
            .bind(id)
            .bind(capture.received_at)
            .bind(capture.method.as_str())
            .bind(capture.url.as_str())
            .bind(capture.path.as_str())
            .bind(capture.host.as_str())
            .bind(capture.client_ip.to_string())
            .bind(prepared.headers.as_str())
            .bind(prepared.query.as_str())
            .bind(prepared.form.as_deref())
            .bind(capture.content_type.as_deref())
            .bind(prepared.body_size)
            .bind(prepared.stored_bytes)
            .bind(override_status)
            .bind(final_status)
            .bind(shape.kind.as_str())
            .bind(prepared.meta.as_str())
            .bind(shape.verification.map(Verification::as_str))
            .execute(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)
    );

    attempt!(
        tx,
        sqlx::query(INSERT_BODY)
            .bind(id)
            .bind(&capture.body[..])
            .bind(prepared.search_text.as_str())
            .execute(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)
    );

    if shape.kind == CaptureKind::Http {
        attempt!(
            tx,
            crate::forward::insert_rule_jobs(&mut tx, &capture.endpoint_id, id, &capture.method)
                .await
                .map_err(CaptureStoreError::classify)
        );
        attempt!(
            tx,
            crate::notify::insert_capture_deliveries(
                &mut tx,
                &capture.endpoint_id,
                id,
                &capture.headers
            )
            .await
            .map_err(CaptureStoreError::classify)
        );
    }

    let payload = attempt!(tx, event_payload(capture, seq, stored_status, shape.kind));
    attempt!(
        tx,
        sqlx::query(NOTIFY)
            .bind(EVENT_CHANNEL)
            .bind(payload)
            .execute(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)
    );

    tx.commit().await.map_err(CaptureStoreError::classify)?;
    Ok(CaptureOutcome::Stored {
        endpoint: Box::new(endpoint),
        capture: StoredCapture {
            id: capture.id,
            seq,
            received_at: capture.received_at,
            response_status,
            body_size: capture.body.len(),
        },
    })
}

fn verifier_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<VerifierSource>, CaptureStoreError> {
    let text: Option<String> = row
        .try_get("verifier")
        .map_err(CaptureStoreError::classify)?;
    let Some(text) = text else {
        return Ok(None);
    };
    let key_id: Option<i16> = row.try_get("key_id").map_err(CaptureStoreError::classify)?;
    let nonce: Option<Vec<u8>> = row.try_get("nonce").map_err(CaptureStoreError::classify)?;
    let ciphertext: Option<Vec<u8>> = row
        .try_get("ciphertext")
        .map_err(CaptureStoreError::classify)?;
    let Ok(verifier) = serde_json::from_str::<VerifierDocument>(&text)
        .map_err(|_| ())
        .and_then(|document| Verifier::from_document(&document).map_err(|_| ()))
    else {
        return Ok(Some(VerifierSource::Unusable));
    };
    let sealed = match (key_id, nonce, ciphertext) {
        (Some(key_id), Some(nonce), Some(ciphertext)) => {
            let opened = KeyId::from_column(key_id)
                .ok()
                .and_then(|key_id| SealedSecret::from_parts(key_id, &nonce, ciphertext).ok());
            match opened {
                Some(sealed) => Some(sealed),
                None => return Ok(Some(VerifierSource::Unusable)),
            }
        }
        _ => None,
    };
    Ok(Some(VerifierSource::Ready { verifier, sealed }))
}

fn endpoint_from_row(
    row: &sqlx::postgres::PgRow,
    id: &EndpointId,
) -> Result<Endpoint, CaptureStoreError> {
    let status: i16 = row
        .try_get("response_status")
        .map_err(CaptureStoreError::classify)?;
    let status = HttpStatus::new(i64::from(status))
        .ok_or_else(|| CaptureStoreError::Corrupt(format!("response status {status}")))?;
    let headers: String = row
        .try_get("response_headers")
        .map_err(CaptureStoreError::classify)?;
    let headers = serde_json::from_str::<Vec<NameValue>>(&headers)
        .map_err(|error| CaptureStoreError::Corrupt(format!("response headers: {error}")))?;
    let delay: i32 = row
        .try_get("response_delay_ms")
        .map_err(CaptureStoreError::classify)?;
    let delay_ms = u32::try_from(delay)
        .map_err(|_| CaptureStoreError::Corrupt(format!("response delay {delay}")))?;
    Ok(Endpoint {
        id: id.clone(),
        status,
        content_type: row
            .try_get("response_content_type")
            .map_err(CaptureStoreError::classify)?,
        headers,
        body: row
            .try_get("response_body")
            .map_err(CaptureStoreError::classify)?,
        delay_ms,
        cors: row.try_get("cors").map_err(CaptureStoreError::classify)?,
        title: row.try_get("title").map_err(CaptureStoreError::classify)?,
        templated: row
            .try_get("response_templated")
            .map_err(CaptureStoreError::classify)?,
        status_template: row
            .try_get("response_status_template")
            .map_err(CaptureStoreError::classify)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(path: String, query_string: String, content_type: String) -> NewCapture {
        NewCapture {
            id: CaptureId::generate().expect("entropy"),
            endpoint_id: EndpointId::generate().expect("entropy"),
            received_at: OffsetDateTime::UNIX_EPOCH,
            method: "POST".to_owned(),
            url: String::new(),
            path,
            query_string,
            host: String::new(),
            client_ip: "2001:db8::1".parse().expect("address"),
            headers: Vec::new(),
            query: Vec::new(),
            form: None,
            content_type: Some(content_type),
            body: Bytes::new(),
            status_override: HttpStatus::new(404),
        }
    }

    fn payload_of(capture: &NewCapture) -> serde_json::Value {
        let payload = capture_event_payload(capture, 7, HttpStatus::default()).expect("fits");
        assert!(payload.len() < 8000, "{} bytes", payload.len());
        serde_json::from_str(&payload).expect("JSON")
    }

    #[test]
    fn one_mebibyte_path_is_cut_to_one_kibibyte() {
        let event = payload_of(&capture(
            format!("/{}", "p".repeat(1 << 20)),
            String::new(),
            "text/plain".to_owned(),
        ));
        assert_eq!(event["s"]["path"].as_str().expect("path").len(), 1024);
    }

    #[test]
    fn escape_heavy_and_multibyte_fields_still_fit() {
        let event = payload_of(&capture(
            "\"".repeat(1 << 20),
            "\\".repeat(1 << 20),
            "\u{20ac}".repeat(100_000),
        ));
        assert_eq!(event["t"], "capture");
        assert_eq!(event["seq"], 7);
    }

    #[test]
    fn truncation_never_splits_a_character() {
        let event = payload_of(&capture(
            "\u{1F600}".repeat(1000),
            String::new(),
            "x".to_owned(),
        ));
        let path = event["s"]["path"].as_str().expect("path");
        assert_eq!(path.len(), 1024);
        assert!(path.chars().all(|character| character == '\u{1F600}'));
    }

    #[test]
    fn nul_bytes_are_replaced_before_storage() {
        let mut stored = capture("/a\0b".to_owned(), "q=\0".to_owned(), "x".to_owned());
        stored.headers.push(NameValue {
            name: "n\0".to_owned(),
            value: "\0v".to_owned(),
        });
        stored.scrub();
        assert_eq!(stored.path, "/a\u{FFFD}b");
        assert_eq!(stored.headers[0].value, "\u{FFFD}v");
        assert!(!stored.search_text().contains(NUL));
    }
}
