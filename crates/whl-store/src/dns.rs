use std::net::IpAddr;
use std::time::Duration;

use serde::Serialize;
use sqlx::Row;
use time::OffsetDateTime;
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;

use crate::captures::{
    CaptureStoreError, EVENT_CHANNEL, EVENT_PAYLOAD_LIMIT, EVICT_OLDEST, INSERT_BODY,
    MAX_EVICTIONS_PER_CAPTURE, NOTIFY, SESSION_LIMITS, TRANSACTION_DEADLINE, format_timestamp,
    scrub_string, to_i32, truncate,
};
use crate::kind::CaptureKind;
use crate::store::Store;

const DNS_ENDPOINT_LIVE: &str = "SELECT dns_enabled FROM endpoints \
     WHERE id = $1 AND deleted_at IS NULL \
     AND last_active_at > now() - make_interval(secs => $2)";

const CLAIM_DNS_SLOT: &str = "UPDATE endpoints SET \
       capture_count = capture_count + 1, \
       bytes_used = bytes_used + $2, \
       last_seq = last_seq + 1, \
       last_active_at = GREATEST(last_active_at, now()) \
     WHERE id = $1 AND deleted_at IS NULL AND dns_enabled \
       AND NOT (overflow <> 'ring' AND COALESCE((SELECT u.storage_over_quota FROM users u WHERE u.id = endpoints.owner_id), false)) \
       AND last_active_at > now() - make_interval(secs => $3) \
       AND (capture_count < $4 OR overflow = 'ring') \
     RETURNING last_seq, capture_count";

const INSERT_DNS_CAPTURE: &str = "INSERT INTO captures \
     (endpoint_id, seq, id, received_at, method, url, path, host, client_ip, headers, query, \
      form, content_type, body_size, stored_bytes, status_override, response_status, kind, meta) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text::inet, '[]'::jsonb, '[]'::jsonb, \
      NULL, NULL, 0, $10, NULL, 0, $11, $12::text::jsonb)";

const SUMMARY_NAME_BYTES: usize = 256;
const RESPONSE_STATUS_NOERROR: u16 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DnsTransport {
    Udp,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DnsFlags {
    pub rd: bool,
    pub cd: bool,
    #[serde(rename = "do")]
    pub dnssec_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DnsMeta {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub class: String,
    pub transport: DnsTransport,
    pub dns_id: u16,
    pub flags: DnsFlags,
    pub edns_udp_size: Option<u16>,
    pub client_subnet: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewDnsCapture {
    pub id: CaptureId,
    pub endpoint_id: EndpointId,
    pub received_at: OffsetDateTime,
    pub path: String,
    pub zone: String,
    pub resolver: IpAddr,
    pub meta: DnsMeta,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DnsCapturePolicy {
    pub max_captures: u32,
    pub retention: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsCaptureOutcome {
    Stored { seq: i64 },
    Gone,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsEndpointState {
    Enabled,
    Disabled,
    Gone,
}

impl NewDnsCapture {
    fn scrub(&mut self) {
        scrub_string(&mut self.path);
        scrub_string(&mut self.zone);
        scrub_string(&mut self.meta.name);
        scrub_string(&mut self.meta.record_type);
        scrub_string(&mut self.meta.class);
        if let Some(subnet) = &mut self.meta.client_subnet {
            scrub_string(subnet);
        }
    }

    fn url(&self) -> String {
        format!("dns:{}", self.meta.name)
    }

    fn search_text(&self) -> String {
        let mut text = format!(
            "{}\n{}\n{}\n{}",
            self.meta.name, self.meta.record_type, self.meta.class, self.resolver
        );
        text.make_ascii_lowercase();
        text
    }

    fn event_payload(&self, seq: i64) -> Result<String, CaptureStoreError> {
        let payload = serde_json::json!({
            "t": "capture",
            "e": self.endpoint_id.unpack(),
            "seq": seq,
            "s": {
                "id": self.id.to_string(),
                "seq": seq,
                "endpoint_id": self.endpoint_id.unpack(),
                "received_at": format_timestamp(self.received_at)?,
                "method": truncate(&self.meta.record_type, 32),
                "path": truncate(&self.path, SUMMARY_NAME_BYTES),
                "query_string": "",
                "client_ip": self.resolver.to_string(),
                "content_type": null,
                "body_size": 0,
                "status_override": null,
                "response_status": RESPONSE_STATUS_NOERROR,
                "forward_state": null,
                "kind": CaptureKind::Dns.as_str(),
            },
        })
        .to_string();
        if payload.len() > EVENT_PAYLOAD_LIMIT {
            return Err(CaptureStoreError::PayloadTooLarge);
        }
        Ok(payload)
    }
}

impl Store {
    pub async fn dns_endpoint_state(
        &self,
        id: &EndpointId,
        retention: Duration,
    ) -> Result<DnsEndpointState, CaptureStoreError> {
        let mut connection = self.acquire_capture_connection().await?;
        let query = sqlx::query(DNS_ENDPOINT_LIVE)
            .bind(id.unpack())
            .bind(retention.as_secs_f64())
            .fetch_optional(&mut *connection);
        let row = tokio::time::timeout(TRANSACTION_DEADLINE, query)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
            .map_err(CaptureStoreError::classify)?;
        let Some(row) = row else {
            return Ok(DnsEndpointState::Gone);
        };
        let enabled: bool = row
            .try_get("dns_enabled")
            .map_err(CaptureStoreError::classify)?;
        Ok(if enabled {
            DnsEndpointState::Enabled
        } else {
            DnsEndpointState::Disabled
        })
    }

    pub async fn set_dns_enabled(
        &self,
        id: &EndpointId,
        enabled: bool,
    ) -> Result<bool, CaptureStoreError> {
        let changed = sqlx::query(
            "UPDATE endpoints SET dns_enabled = $2 WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id.unpack())
        .bind(enabled)
        .execute(self.pool())
        .await
        .map_err(CaptureStoreError::classify)?;
        Ok(changed.rows_affected() == 1)
    }

    pub async fn store_dns_capture(
        &self,
        mut capture: NewDnsCapture,
        policy: DnsCapturePolicy,
    ) -> Result<DnsCaptureOutcome, CaptureStoreError> {
        capture.scrub();
        let meta = serde_json::to_string(&capture.meta)?;
        let search_text = capture.search_text();
        let stored = capture.url().len()
            + capture.path.len()
            + capture.zone.len()
            + meta.len()
            + search_text.len();
        let stored_bytes = to_i32(stored, "stored bytes")?;
        let work = self.execute_dns_capture(capture, policy, meta, search_text, stored_bytes);
        tokio::time::timeout(TRANSACTION_DEADLINE, work)
            .await
            .map_err(|_| CaptureStoreError::DeadlineExceeded)?
    }

    async fn execute_dns_capture(
        &self,
        capture: NewDnsCapture,
        policy: DnsCapturePolicy,
        meta: String,
        search_text: String,
        stored_bytes: i32,
    ) -> Result<DnsCaptureOutcome, CaptureStoreError> {
        let mut tx = self.begin_capture_transaction().await?;
        let outcome =
            run_dns_capture(&mut tx, &capture, policy, &meta, &search_text, stored_bytes).await;
        match outcome {
            Ok(DnsCaptureOutcome::Stored { seq }) => {
                tx.commit().await.map_err(CaptureStoreError::classify)?;
                Ok(DnsCaptureOutcome::Stored { seq })
            }
            Ok(other) => {
                tx.rollback().await.map_err(CaptureStoreError::classify)?;
                Ok(other)
            }
            Err(error) => {
                if let Err(rollback) = tx.rollback().await {
                    tracing::warn!(%rollback, "rollback after a failed DNS capture also failed");
                }
                Err(error)
            }
        }
    }
}

async fn run_dns_capture(
    tx: &mut sqlx::Transaction<'static, sqlx::Postgres>,
    capture: &NewDnsCapture,
    policy: DnsCapturePolicy,
    meta: &str,
    search_text: &str,
    stored_bytes: i32,
) -> Result<DnsCaptureOutcome, CaptureStoreError> {
    let max_captures = i64::from(policy.max_captures);
    sqlx::query(SESSION_LIMITS)
        .execute(&mut **tx)
        .await
        .map_err(CaptureStoreError::classify)?;

    let claimed = sqlx::query(CLAIM_DNS_SLOT)
        .bind(capture.endpoint_id.unpack())
        .bind(i64::from(stored_bytes))
        .bind(policy.retention.as_secs_f64())
        .bind(max_captures)
        .fetch_optional(&mut **tx)
        .await
        .map_err(CaptureStoreError::classify)?;
    let Some(row) = claimed else {
        let live = sqlx::query(DNS_ENDPOINT_LIVE)
            .bind(capture.endpoint_id.unpack())
            .bind(policy.retention.as_secs_f64())
            .fetch_optional(&mut **tx)
            .await
            .map_err(CaptureStoreError::classify)?;
        let enabled = match live {
            Some(row) => row
                .try_get::<bool, _>("dns_enabled")
                .map_err(CaptureStoreError::classify)?,
            None => false,
        };
        return Ok(if enabled {
            DnsCaptureOutcome::Full
        } else {
            DnsCaptureOutcome::Gone
        });
    };

    let seq: i64 = row
        .try_get("last_seq")
        .map_err(CaptureStoreError::classify)?;
    let count_after: i32 = row
        .try_get("capture_count")
        .map_err(CaptureStoreError::classify)?;
    let surplus = i64::from(count_after) - max_captures;
    if surplus > 0 {
        sqlx::query(EVICT_OLDEST)
            .bind(capture.endpoint_id.unpack())
            .bind(surplus.min(MAX_EVICTIONS_PER_CAPTURE))
            .execute(&mut **tx)
            .await
            .map_err(CaptureStoreError::classify)?;
    }

    let id = capture.id.unpack();
    sqlx::query(INSERT_DNS_CAPTURE)
        .bind(capture.endpoint_id.unpack())
        .bind(seq)
        .bind(id)
        .bind(capture.received_at)
        .bind(capture.meta.record_type.as_str())
        .bind(capture.url())
        .bind(capture.path.as_str())
        .bind(capture.zone.as_str())
        .bind(capture.resolver.to_string())
        .bind(stored_bytes)
        .bind(CaptureKind::Dns.as_str())
        .bind(meta)
        .execute(&mut **tx)
        .await
        .map_err(CaptureStoreError::classify)?;

    sqlx::query(INSERT_BODY)
        .bind(id)
        .bind(&[] as &[u8])
        .bind(search_text)
        .execute(&mut **tx)
        .await
        .map_err(CaptureStoreError::classify)?;

    let payload = capture.event_payload(seq)?;
    sqlx::query(NOTIFY)
        .bind(EVENT_CHANNEL)
        .bind(payload)
        .execute(&mut **tx)
        .await
        .map_err(CaptureStoreError::classify)?;
    Ok(DnsCaptureOutcome::Stored { seq })
}
