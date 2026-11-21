use std::net::IpAddr;
use std::time::Duration;

use bytes::Bytes;
use serde_json::Value;
use sqlx::Row;
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;
use whl_core::status::HttpStatus;

use crate::capture_queries::{CaptureError, classify, notify};
use crate::captures::{CapturePolicy, CaptureStoreError, Form, NameValue, NewCapture, Prepared};
use crate::kind::CaptureKind;
use crate::store::Store;

pub const IMPORT_BATCH: usize = 50;
pub const IMPORT_BATCH_DEADLINE: Duration = Duration::from_secs(30);

const NUL: char = '\0';
const REPLACEMENT: char = '\u{FFFD}';

const SESSION_LIMITS: &str = "SELECT \
       set_config('statement_timeout', '20s', true), \
       set_config('idle_in_transaction_session_timeout', '30s', true)";

const LOCK_FOR_IMPORT: &str = "SELECT capture_count::bigint AS capture_count, \
       COALESCE(max_captures::bigint, $3) AS effective_max \
     FROM endpoints \
     WHERE id = $1 AND deleted_at IS NULL \
       AND (never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds::float8, $2))) \
     FOR UPDATE";

const CLAIM_SLOTS: &str = "UPDATE endpoints SET \
       capture_count = capture_count + $2, \
       bytes_used = bytes_used + $3, \
       last_seq = last_seq + $2, \
       last_active_at = GREATEST(last_active_at, now()) \
     WHERE id = $1 \
     RETURNING last_seq";

const INSERT_IMPORTED: &str = "INSERT INTO captures \
     (endpoint_id, seq, id, received_at, method, url, path, host, client_ip, headers, query, \
      form, content_type, body_size, stored_bytes, status_override, response_status, kind, meta, \
      imported) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text::inet, $10::text::jsonb, $11::text::jsonb, \
      $12::text::jsonb, $13, $14, $15, $16, $17, $18, $19::text::jsonb, true)";

const INSERT_BODY: &str =
    "INSERT INTO capture_bodies (capture_id, data, search_text) VALUES ($1, $2, $3)";

#[derive(Debug, Clone)]
pub struct ImportCapture {
    pub original_id: CaptureId,
    pub received_at: OffsetDateTime,
    pub kind: CaptureKind,
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
    pub body: Vec<u8>,
    pub status_override: Option<HttpStatus>,
    pub response_status: HttpStatus,
    pub meta: Value,
}

impl ImportCapture {
    pub fn approx_bytes(&self) -> usize {
        let pairs = |pairs: &[NameValue]| {
            pairs
                .iter()
                .map(|pair| pair.name.len() + pair.value.len())
                .sum::<usize>()
        };
        self.body.len()
            + self.url.len()
            + self.path.len()
            + self.query_string.len()
            + self.host.len()
            + pairs(&self.headers)
            + pairs(&self.query)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStop {
    Capacity,
    Storage,
}

impl ImportStop {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::Storage => "storage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportedBatch {
    pub imported: usize,
    pub stored_bytes: u64,
    pub stop: Option<ImportStop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    Stored(ImportedBatch),
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportPlacement {
    pub keep_times: bool,
    pub imported_at: OffsetDateTime,
}

pub fn storage_headroom(used: u64, cap: u64) -> u64 {
    cap.saturating_sub(used)
}

fn scrub_meta(value: &mut Value) {
    match value {
        Value::String(text) => {
            if text.contains(NUL) {
                *text = text.replace(NUL, &REPLACEMENT.to_string());
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub_meta),
        Value::Object(map) => {
            let entries = std::mem::take(map);
            for (key, mut item) in entries {
                scrub_meta(&mut item);
                let key = if key.contains(NUL) {
                    key.replace(NUL, &REPLACEMENT.to_string())
                } else {
                    key
                };
                map.insert(key, item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

struct Staged {
    capture: NewCapture,
    prepared: Prepared,
    kind: CaptureKind,
    status_override: Option<i16>,
    response_status: i16,
    meta: String,
}

fn stage(
    endpoint: &EndpointId,
    item: ImportCapture,
    placement: ImportPlacement,
) -> Result<Staged, CaptureStoreError> {
    let id = CaptureId::generate()
        .map_err(|error| CaptureStoreError::Corrupt(format!("capture id generation: {error}")))?;
    let mut meta = match item.meta {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    meta.insert(
        "original_id".to_owned(),
        Value::String(item.original_id.to_string()),
    );
    let mut meta = Value::Object(meta);
    scrub_meta(&mut meta);
    let meta = serde_json::to_string(&meta)?;
    let status_override = item
        .status_override
        .map(|status| i16::try_from(status.unpack()))
        .transpose()
        .map_err(|_| CaptureStoreError::OutOfRange("status override"))?;
    let response_status = i16::try_from(item.response_status.unpack())
        .map_err(|_| CaptureStoreError::OutOfRange("response status"))?;
    let received_at = if placement.keep_times {
        item.received_at
    } else {
        placement.imported_at
    };
    let mut capture = NewCapture {
        id,
        endpoint_id: endpoint.clone(),
        received_at,
        method: item.method,
        url: item.url,
        path: item.path,
        query_string: item.query_string,
        host: item.host,
        client_ip: item.client_ip,
        headers: item.headers,
        query: item.query,
        form: item.form,
        content_type: item.content_type,
        body: Bytes::from(item.body),
        status_override: item.status_override,
    };
    capture.scrub();
    let mut prepared = Prepared::new(&capture)?;
    let meta_bytes =
        i32::try_from(meta.len()).map_err(|_| CaptureStoreError::OutOfRange("meta size"))?;
    prepared.stored_bytes = prepared
        .stored_bytes
        .checked_add(meta_bytes)
        .ok_or(CaptureStoreError::OutOfRange("stored bytes"))?;
    Ok(Staged {
        capture,
        prepared,
        kind: item.kind,
        status_override,
        response_status,
        meta,
    })
}

impl Store {
    pub async fn import_batch(
        &self,
        endpoint: &EndpointId,
        captures: Vec<ImportCapture>,
        placement: ImportPlacement,
        policy: CapturePolicy,
        byte_budget: u64,
    ) -> Result<ImportOutcome, CaptureStoreError> {
        let staged = captures
            .into_iter()
            .map(|item| stage(endpoint, item, placement))
            .collect::<Result<Vec<_>, _>>()?;
        let work = self.write_batch(endpoint, staged, policy, byte_budget);
        tokio::time::timeout(IMPORT_BATCH_DEADLINE, work)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
    }

    async fn write_batch(
        &self,
        endpoint: &EndpointId,
        staged: Vec<Staged>,
        policy: CapturePolicy,
        byte_budget: u64,
    ) -> Result<ImportOutcome, CaptureStoreError> {
        let mut tx = self.begin_capture_transaction().await?;
        sqlx::query(SESSION_LIMITS)
            .execute(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        let locked = sqlx::query(LOCK_FOR_IMPORT)
            .bind(endpoint.unpack())
            .bind(policy.retention.as_secs_f64())
            .bind(i64::from(policy.max_captures))
            .fetch_optional(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        let Some(row) = locked else {
            tx.rollback().await.map_err(CaptureStoreError::classify)?;
            return Ok(ImportOutcome::Gone);
        };
        let count: i64 = row
            .try_get("capture_count")
            .map_err(CaptureStoreError::classify)?;
        let effective_max: i64 = row
            .try_get("effective_max")
            .map_err(CaptureStoreError::classify)?;
        let room = usize::try_from(effective_max.saturating_sub(count)).unwrap_or(0);

        let mut taken = 0_usize;
        let mut bytes = 0_u64;
        let mut stop = None;
        for item in &staged {
            if taken >= room {
                stop = Some(ImportStop::Capacity);
                break;
            }
            let size = u64::try_from(item.prepared.stored_bytes).unwrap_or(u64::MAX);
            if bytes.saturating_add(size) > byte_budget {
                stop = Some(ImportStop::Storage);
                break;
            }
            bytes += size;
            taken += 1;
        }
        if taken == 0 {
            tx.rollback().await.map_err(CaptureStoreError::classify)?;
            return Ok(ImportOutcome::Stored(ImportedBatch {
                imported: 0,
                stored_bytes: 0,
                stop,
            }));
        }

        let claimed = sqlx::query(CLAIM_SLOTS)
            .bind(endpoint.unpack())
            .bind(i64::try_from(taken).map_err(|_| CaptureStoreError::OutOfRange("batch size"))?)
            .bind(i64::try_from(bytes).map_err(|_| CaptureStoreError::OutOfRange("batch bytes"))?)
            .fetch_one(&mut *tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        let last_seq: i64 = claimed
            .try_get("last_seq")
            .map_err(CaptureStoreError::classify)?;
        let first_seq = last_seq - i64::try_from(taken).unwrap_or(0) + 1;

        for (offset, item) in staged.iter().take(taken).enumerate() {
            let seq = first_seq + i64::try_from(offset).unwrap_or(0);
            let id = item.capture.id.unpack();
            sqlx::query(INSERT_IMPORTED)
                .bind(endpoint.unpack())
                .bind(seq)
                .bind(id)
                .bind(item.capture.received_at)
                .bind(item.capture.method.as_str())
                .bind(item.capture.url.as_str())
                .bind(item.capture.path.as_str())
                .bind(item.capture.host.as_str())
                .bind(item.capture.client_ip.to_string())
                .bind(item.prepared.headers.as_str())
                .bind(item.prepared.query.as_str())
                .bind(item.prepared.form.as_deref())
                .bind(item.capture.content_type.as_deref())
                .bind(item.prepared.body_size)
                .bind(item.prepared.stored_bytes)
                .bind(item.status_override)
                .bind(item.response_status)
                .bind(item.kind.as_str())
                .bind(item.meta.as_str())
                .execute(&mut *tx)
                .await
                .map_err(CaptureStoreError::classify)?;
            sqlx::query(INSERT_BODY)
                .bind(id)
                .bind(&item.capture.body[..])
                .bind(item.prepared.search_text.as_str())
                .execute(&mut *tx)
                .await
                .map_err(CaptureStoreError::classify)?;
        }
        tx.commit().await.map_err(CaptureStoreError::classify)?;
        Ok(ImportOutcome::Stored(ImportedBatch {
            imported: taken,
            stored_bytes: bytes,
            stop,
        }))
    }

    pub async fn announce_import(
        &self,
        endpoint: &EndpointId,
        imported: usize,
    ) -> Result<(), CaptureError> {
        let mut tx = self.pool().begin().await.map_err(classify)?;
        let payload = serde_json::json!({
            "t": "captures_imported",
            "e": endpoint.unpack(),
            "d": { "imported": imported },
        });
        notify(&mut tx, &payload).await?;
        tx.commit().await.map_err(classify)?;
        Ok(())
    }
}
