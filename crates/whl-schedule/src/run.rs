use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http::{HeaderName, HeaderValue};
use time::OffsetDateTime;
use url::Url;
use whl_core::capture_id::CaptureId;
use whl_forward::{OutboundRequest, OutboundResponse, validate_target_url};
use whl_secrets::{MIN_REDACTED_VALUE_BYTES, OVERSIZE_PLACEHOLDER, Redactor, Secret, SecretName};
use whl_store::captures::{CaptureOutcome, CapturePolicy, CaptureShape, NameValue, NewCapture};
use whl_store::schedules::{ClaimedSchedule, RunOutcome};
use whl_store::{CaptureKind, Uuid};

use crate::outcome::{is_success, run_error};
use crate::render::{RenderScene, render_request};
use crate::request::CompiledRequest;
use crate::runner::Inner;

pub const HOP_HEADER: HeaderName = HeaderName::from_static("x-webhooks-live-hop");
pub const OUTGOING_HOP: HeaderValue = HeaderValue::from_static("1");
pub const QUOTA_EXCEEDED: &str = "quota_exceeded: the hourly run limit of the owner is used up";
pub const STORAGE_FULL: &str = "storage_full: the instance is not accepting captures";
pub const SECRET_TOO_SHORT: &str = "secret_too_short: a secret used by this schedule is shorter than 4 bytes and cannot be redacted";

const SEND_GRACE: Duration = Duration::from_secs(1);
const SECONDS_PER_HOUR: i64 = 3600;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Aborted(pub String);

enum Stop {
    Failure(String),
    Pause(String),
    Aborted(Aborted),
}

impl From<Aborted> for Stop {
    fn from(aborted: Aborted) -> Self {
        Self::Aborted(aborted)
    }
}

#[derive(Debug)]
pub struct Executed {
    pub outcome: RunOutcome,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub capture_id: Option<Uuid>,
    pub duration_ms: i32,
    pub pause: Option<String>,
}

struct Observed {
    method: String,
    url: String,
    response: Option<OutboundResponse>,
    error: Option<String>,
}

struct Scrubber {
    redactor: Option<Redactor>,
}

impl Scrubber {
    fn text(&self, text: &str) -> String {
        match &self.redactor {
            Some(redactor) => redactor.redact_str(text).into_owned(),
            None => text.to_owned(),
        }
    }

    fn body(&self, body: Bytes) -> Bytes {
        let Some(redactor) = &self.redactor else {
            return body;
        };
        let text = String::from_utf8_lossy(&body);
        match redactor.redact_str(&text) {
            Cow::Owned(changed) => Bytes::from(changed),
            Cow::Borrowed(kept) if kept == OVERSIZE_PLACEHOLDER && text != OVERSIZE_PLACEHOLDER => {
                Bytes::from_static(OVERSIZE_PLACEHOLDER.as_bytes())
            }
            Cow::Borrowed(_) => body,
        }
    }
}

pub(crate) async fn execute(
    inner: &Inner,
    claimed: &ClaimedSchedule,
    started: OffsetDateTime,
) -> Result<Executed, Aborted> {
    let begun = Instant::now();
    if inner.storage_full.is_full() {
        return Ok(skipped(STORAGE_FULL, begun));
    }
    let Some(owner) = claimed.owner_id else {
        return Ok(failed_before_request(
            "the endpoint of the schedule has no owner",
            begun,
        ));
    };
    let admitted = inner
        .store
        .consume_schedule_quota(owner, hour_start(started), inner.config.runs_per_hour)
        .await
        .map_err(|error| Aborted(format!("cannot count the run against the quota: {error}")))?;
    if !admitted {
        return Ok(skipped(QUOTA_EXCEEDED, begun));
    }

    let mut observed = Observed {
        method: claimed.request.method.clone(),
        url: claimed.request.url.clone(),
        response: None,
        error: None,
    };
    let mut scrubber = Scrubber { redactor: None };
    match perform(inner, claimed, started, &mut observed, &mut scrubber).await {
        Ok(()) => {}
        Err(Stop::Failure(message)) => observed.error = Some(message),
        Err(Stop::Pause(reason)) => {
            let mut executed = skipped(&reason, begun);
            executed.pause = Some(reason);
            return Ok(executed);
        }
        Err(Stop::Aborted(aborted)) => return Err(aborted),
    }
    Ok(conclude(inner, claimed, started, observed, &scrubber, begun).await)
}

fn skipped(reason: &str, begun: Instant) -> Executed {
    Executed {
        outcome: RunOutcome::Skipped,
        status: None,
        error: Some(reason.to_owned()),
        capture_id: None,
        duration_ms: elapsed_ms(begun),
        pause: None,
    }
}

fn failed_before_request(message: &str, begun: Instant) -> Executed {
    Executed {
        outcome: RunOutcome::Failed,
        status: None,
        error: Some(run_error(message)),
        capture_id: None,
        duration_ms: elapsed_ms(begun),
        pause: None,
    }
}

async fn perform(
    inner: &Inner,
    claimed: &ClaimedSchedule,
    started: OffsetDateTime,
    observed: &mut Observed,
    scrubber: &mut Scrubber,
) -> Result<(), Stop> {
    let compiled = CompiledRequest::compile(&claimed.request).map_err(|violations| {
        let first = violations.first().map_or_else(
            || "the request is invalid".to_owned(),
            |violation| format!("{}: {}", violation.field, violation.message),
        );
        Stop::Failure(format!("the stored request is invalid: {first}"))
    })?;
    let secrets = open_secrets(inner, claimed, &compiled).await?;
    if secrets
        .iter()
        .any(|(_, secret)| secret.unpack().len() < MIN_REDACTED_VALUE_BYTES)
    {
        return Err(Stop::Pause(SECRET_TOO_SHORT.to_owned()));
    }
    if !secrets.is_empty() {
        let redactor = Redactor::new(secrets.iter().map(|(name, secret)| (name, secret)))
            .map_err(|error| Stop::Failure(format!("the secrets cannot be redacted: {error}")))?;
        scrubber.redactor = Some(redactor);
    }
    let scene = RenderScene {
        now: started,
        endpoint_id: claimed.endpoint_id.unpack(),
        public_url: &inner.config.public_url,
    };
    let rendered = render_request(&compiled, scene, &secrets).map_err(Stop::Failure)?;
    observed.url = rendered.url.clone();
    let target = validate_target_url(&rendered.url, inner.client.config())
        .map_err(|error| Stop::Failure(error.to_string()))?;
    let mut headers = rendered.headers;
    headers.insert(HOP_HEADER, OUTGOING_HOP);
    let request = OutboundRequest::new(compiled.method, target, headers, rendered.body);
    let deadline = inner
        .client
        .config()
        .total_timeout
        .saturating_add(SEND_GRACE);
    let sent = tokio::time::timeout(deadline, inner.client.send(request))
        .await
        .map_err(|_| Stop::Failure("the request did not finish in time".to_owned()))?;
    let response = sent.map_err(|error| Stop::Failure(error.to_string()))?;
    observed.response = Some(response);
    Ok(())
}

async fn open_secrets(
    inner: &Inner,
    claimed: &ClaimedSchedule,
    compiled: &CompiledRequest,
) -> Result<Vec<(SecretName, Secret)>, Stop> {
    let names = compiled.secret_names();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let Some(keyring) = &inner.keyring else {
        return Err(Stop::Failure(
            "the request uses secrets but secrets are not available on this instance".to_owned(),
        ));
    };
    let mut opened = Vec::with_capacity(names.len());
    for raw in names {
        let name = SecretName::parse(&raw)
            .map_err(|_| Stop::Failure(format!("secret {raw} has an invalid name")))?;
        let sealed = inner
            .store
            .get_sealed_secret(&claimed.endpoint_id, &name)
            .await
            .map_err(|error| {
                if error.is_unavailable() {
                    Stop::Aborted(Aborted(format!("cannot read secret {name}: {error}")))
                } else {
                    Stop::Failure(format!("secret {name} cannot be read"))
                }
            })?
            .ok_or_else(|| Stop::Failure(format!("secret {name} does not exist")))?;
        let secret = keyring
            .open(&claimed.endpoint_id, &name, &sealed)
            .map_err(|_| Stop::Failure(format!("secret {name} cannot be opened")))?;
        opened.push((name, secret));
    }
    Ok(opened)
}

async fn conclude(
    inner: &Inner,
    claimed: &ClaimedSchedule,
    started: OffsetDateTime,
    observed: Observed,
    scrubber: &Scrubber,
    begun: Instant,
) -> Executed {
    let duration_ms = elapsed_ms(begun);
    let status = observed
        .response
        .as_ref()
        .map(|response| response.status.as_u16());
    let mut error = observed
        .error
        .as_deref()
        .map(|message| scrubber.text(message));
    let mut capture_id = None;
    match build_capture(
        claimed,
        started,
        &observed,
        scrubber,
        duration_ms,
        error.as_deref(),
    ) {
        Ok((capture, shape)) => {
            let policy = CapturePolicy {
                max_captures: inner.config.max_captures,
                retention: inner.config.retention,
            };
            match inner.store.store_capture_as(capture, policy, shape).await {
                Ok(CaptureOutcome::Stored { capture, .. }) => {
                    capture_id = Some(capture.id.unpack());
                }
                Ok(CaptureOutcome::Full) => {
                    error.get_or_insert_with(|| {
                        "endpoint_full: the capture was not stored".to_owned()
                    });
                }
                Ok(CaptureOutcome::Gone) => {
                    error.get_or_insert_with(|| {
                        "endpoint_gone: the capture was not stored".to_owned()
                    });
                }
                Err(store_error) => {
                    tracing::warn!(schedule = %claimed.id, %store_error, "cannot store a schedule capture");
                    error.get_or_insert_with(|| {
                        "capture_not_stored: the capture was not stored".to_owned()
                    });
                }
            }
        }
        Err(message) => {
            error.get_or_insert(message);
        }
    }
    let status_failed = status.is_some_and(|status| !is_success(status));
    if status_failed && error.is_none() {
        error = status.map(|status| format!("HTTP {status}"));
    }
    let failed = error.is_some() || status_failed || status.is_none();
    Executed {
        outcome: if failed {
            RunOutcome::Failed
        } else {
            RunOutcome::Succeeded
        },
        status,
        error: error.map(|message| run_error(&message)),
        capture_id,
        duration_ms,
        pause: None,
    }
}

fn build_capture(
    claimed: &ClaimedSchedule,
    started: OffsetDateTime,
    observed: &Observed,
    scrubber: &Scrubber,
    duration_ms: i32,
    error: Option<&str>,
) -> Result<(NewCapture, CaptureShape), String> {
    let id = CaptureId::generate().map_err(|_| "capture_not_stored: no capture id".to_owned())?;
    let url = scrubber.text(&observed.url);
    let host = Url::parse(&url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .unwrap_or_default();
    let final_url = observed.response.as_ref().map(|response| {
        if response.redirects_followed == 0 {
            url.clone()
        } else {
            scrubber.text(response.final_url.unpack().as_str())
        }
    });
    let (headers, content_type, body, truncated, status) = match &observed.response {
        Some(response) => {
            let headers: Vec<NameValue> = response
                .headers
                .iter()
                .map(|header| NameValue {
                    name: header.name.as_str().to_owned(),
                    value: scrubber.text(&header.value),
                })
                .collect();
            let content_type = headers
                .iter()
                .find(|header| header.name == "content-type")
                .map(|header| header.value.clone());
            (
                headers,
                content_type,
                scrubber.body(response.body.clone()),
                response.truncated,
                response.status.as_u16(),
            )
        }
        None => (Vec::new(), None, Bytes::new(), false, 0),
    };
    let meta = serde_json::json!({
        "schedule_id": claimed.id.hyphenated().to_string(),
        "schedule_name": claimed.name,
        "request": { "method": observed.method, "url": url },
        "duration_ms": duration_ms,
        "final_url": final_url,
        "error": error,
        "truncated": truncated,
    });
    let capture = NewCapture {
        id,
        endpoint_id: claimed.endpoint_id.clone(),
        received_at: started,
        method: observed.method.clone(),
        url,
        path: String::new(),
        query_string: String::new(),
        host,
        client_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        headers,
        query: Vec::new(),
        form: None,
        content_type,
        body,
        status_override: None,
    };
    let shape = CaptureShape {
        kind: CaptureKind::Schedule,
        meta,
        response_status: Some(status),
        verification: None,
    };
    Ok((capture, shape))
}

pub fn hour_start(moment: OffsetDateTime) -> OffsetDateTime {
    let seconds = moment.unix_timestamp();
    OffsetDateTime::from_unix_timestamp(seconds - seconds.rem_euclid(SECONDS_PER_HOUR))
        .unwrap_or(moment)
}

fn elapsed_ms(begun: Instant) -> i32 {
    i32::try_from(begun.elapsed().as_millis()).unwrap_or(i32::MAX)
}
