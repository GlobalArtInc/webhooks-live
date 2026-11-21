use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use http::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use url::Url;
use whl_forward::{ForwardError, HOP_HEADER, OutboundRequest, validate_target_url};
use whl_rules::{FailureCode, OutboundDone, S3Content, StoreS3Call, bound_text};
use whl_store::CaptureDetail;

use crate::http::{DELIVERY_HEADER, describe, is_retryable_error, is_retryable_status};
use crate::ids::delivery_header;
use crate::outbound::{Ctx, StepAttempt, charge, count, failed};
use crate::sigv4::{SERVICE, SignInput, amz_date, encode_path, sha256_hex, sign};

const PURPOSE: &str = "store";
const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const SEND_GRACE: Duration = Duration::from_secs(1);
const STORE_RETRY: Duration = Duration::from_secs(5);
const ERROR_SNIPPET_BYTES: usize = 300;
const JSON_TYPE: &str = "application/json";
const BINARY_TYPE: &str = "application/octet-stream";
const AMZ_DATE: &str = "x-amz-date";
const AMZ_CONTENT_SHA: &str = "x-amz-content-sha256";

struct Content {
    bytes: Vec<u8>,
    content_type: &'static str,
    extension: &'static str,
}

fn capture_json(detail: &CaptureDetail) -> Result<Vec<u8>, serde_json::Error> {
    let summary = &detail.summary;
    let mut body = Map::new();
    body.insert("truncated".to_owned(), Value::Bool(detail.body.truncated));
    match std::str::from_utf8(&detail.body.prefix) {
        Ok(text) => {
            body.insert("encoding".to_owned(), Value::String("utf8".to_owned()));
            body.insert("text".to_owned(), Value::String(text.to_owned()));
        }
        Err(_) => {
            body.insert("encoding".to_owned(), Value::String("base64".to_owned()));
            body.insert(
                "data".to_owned(),
                Value::String(STANDARD.encode(&detail.body.prefix)),
            );
        }
    }
    serde_json::to_vec(&json!({
        "endpoint_id": summary.endpoint_id,
        "capture_id": summary.id,
        "seq": summary.seq,
        "received_at": summary.received_at,
        "method": summary.method,
        "url": detail.url,
        "path": summary.path,
        "query": detail.query,
        "headers": detail.headers,
        "client_ip": summary.client_ip,
        "content_type": summary.content_type,
        "body_size": summary.body_size,
        "body": body,
    }))
}

async fn content(ctx: &Ctx<'_>, call: &StoreS3Call) -> Result<Content, StepAttempt> {
    match call.content {
        S3Content::CaptureJson => match capture_json(ctx.detail) {
            Ok(bytes) => Ok(Content {
                bytes,
                content_type: JSON_TYPE,
                extension: "json",
            }),
            Err(_) => Err(failed(
                ctx.scrub,
                FailureCode::Internal,
                "the capture cannot be encoded",
            )),
        },
        S3Content::Body => {
            let limit = i32::try_from(MAX_BODY_BYTES + 1).unwrap_or(i32::MAX);
            match ctx.store.load_capture_body(ctx.run.capture_id, limit).await {
                Ok(Some(bytes)) if bytes.len() <= MAX_BODY_BYTES => Ok(Content {
                    bytes,
                    content_type: BINARY_TYPE,
                    extension: "bin",
                }),
                Ok(Some(_)) => Err(failed(
                    ctx.scrub,
                    FailureCode::Http,
                    "the body is larger than 4 MiB",
                )),
                Ok(None) => Err(failed(
                    ctx.scrub,
                    FailureCode::Internal,
                    "the capture no longer exists",
                )),
                Err(error) if error.is_unavailable() => {
                    Err(StepAttempt::Retry { after: STORE_RETRY })
                }
                Err(error) => {
                    tracing::warn!(%error, run_id = %ctx.run.id, "a capture body cannot be read");
                    Err(failed(
                        ctx.scrub,
                        FailureCode::Internal,
                        "the capture body cannot be read",
                    ))
                }
            }
        }
    }
}

fn is_dot_segment(segment: &str) -> bool {
    segment == "." || segment == ".."
}

fn object_path(
    base: &Url,
    prefix: &str,
    ctx: &Ctx<'_>,
    extension: &str,
) -> Result<String, &'static str> {
    let base_path = base.path().trim_end_matches('/');
    let plain = base_path.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/')
    });
    if !plain {
        return Err("the bucket URL path may use only letters, digits and - _ . ~ /");
    }
    if base_path.split('/').any(is_dot_segment) {
        return Err("the bucket URL path must not contain dot segments");
    }
    let summary = &ctx.detail.summary;
    let key = format!(
        "{prefix}{}/{}-{}.{extension}",
        ctx.endpoint.unpack(),
        summary.seq,
        summary.id
    );
    if key.split('/').any(is_dot_segment) {
        return Err("the prefix must not contain dot segments");
    }
    Ok(format!("{base_path}/{}", encode_path(&key)))
}

fn authority(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn header(value: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(value).ok()
}

struct RequestParts<'a> {
    content_type: &'a str,
    moment: &'a str,
    payload_hash: &'a str,
    authorization: &'a str,
    position: usize,
}

fn request_headers(ctx: &Ctx<'_>, parts: &RequestParts<'_>) -> Option<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, header(parts.content_type)?);
    headers.insert(HeaderName::from_static(AMZ_DATE), header(parts.moment)?);
    headers.insert(
        HeaderName::from_static(AMZ_CONTENT_SHA),
        header(parts.payload_hash)?,
    );
    let mut authorization = header(parts.authorization)?;
    authorization.set_sensitive(true);
    headers.insert(AUTHORIZATION, authorization);
    headers.insert(
        HeaderName::from_static(HOP_HEADER),
        HeaderValue::from(ctx.hop.saturating_add(1)),
    );
    headers.insert(
        HeaderName::from_static(DELIVERY_HEADER),
        header(&delivery_header(ctx.run.id, parts.position))?,
    );
    headers.insert(USER_AGENT, header(&ctx.client.config().user_agent)?);
    Some(headers)
}

pub(crate) async fn execute(
    ctx: &Ctx<'_>,
    call: &StoreS3Call,
    position: usize,
    attempt: u32,
) -> StepAttempt {
    let (Some(access), Some(secret)) = (
        ctx.opened.get(&call.access_key_secret),
        ctx.opened.get(&call.secret_key_secret),
    ) else {
        return failed(
            ctx.scrub,
            FailureCode::Http,
            "a secret named by the step is not available",
        );
    };
    let base = match Url::parse(&call.bucket_url) {
        Ok(base) if base.query().is_none() && base.fragment().is_none() => base,
        _ => {
            return failed(
                ctx.scrub,
                FailureCode::Http,
                "the bucket URL must be a plain URL without a query or fragment",
            );
        }
    };
    let content = match content(ctx, call).await {
        Ok(content) => content,
        Err(attempt) => return attempt,
    };
    let path = match object_path(&base, &call.prefix, ctx, content.extension) {
        Ok(path) => path,
        Err(reason) => return failed(ctx.scrub, FailureCode::Http, reason),
    };
    let Some(authority) = authority(&base) else {
        return failed(ctx.scrub, FailureCode::Http, "the bucket URL has no host");
    };
    let target = match validate_target_url(
        &format!("{}://{authority}{path}", base.scheme()),
        ctx.client.config(),
    ) {
        Ok(target) => target,
        Err(error) => {
            count(PURPOSE, "blocked");
            return failed(ctx.scrub, FailureCode::Http, &describe(&error));
        }
    };
    if let Err(early) = charge(ctx).await {
        return early;
    }
    let moment = amz_date(OffsetDateTime::now_utc());
    let payload_hash = sha256_hex(&content.bytes);
    let signed_headers = vec![
        ("content-type".to_owned(), content.content_type.to_owned()),
        ("host".to_owned(), authority),
        (AMZ_CONTENT_SHA.to_owned(), payload_hash.clone()),
        (AMZ_DATE.to_owned(), moment.clone()),
    ];
    let signed = match sign(&SignInput {
        method: "PUT",
        canonical_path: &path,
        canonical_query: "",
        headers: &signed_headers,
        payload_hash: &payload_hash,
        region: &call.region,
        service: SERVICE,
        amz_date: &moment,
        access_key: access.unpack(),
        secret_key: secret.unpack(),
    }) {
        Ok(signed) => signed,
        Err(_) => {
            return failed(
                ctx.scrub,
                FailureCode::Internal,
                "the request cannot be signed",
            );
        }
    };
    let Some(headers) = request_headers(
        ctx,
        &RequestParts {
            content_type: content.content_type,
            moment: &moment,
            payload_hash: &payload_hash,
            authorization: &signed.authorization,
            position,
        },
    ) else {
        return failed(
            ctx.scrub,
            FailureCode::Internal,
            "the request headers cannot be built",
        );
    };
    let size = content.bytes.len();
    let request = OutboundRequest::new(Method::PUT, target, headers, Bytes::from(content.bytes));
    let limit = ctx.client.config().total_timeout.saturating_add(SEND_GRACE);
    let sent = tokio::time::timeout(limit, ctx.client.send(request))
        .await
        .unwrap_or(Err(ForwardError::TotalTimeout));
    let policy = ctx.settings.retry;
    match sent {
        Ok(response) => {
            let status = response.status.as_u16();
            if (200..300).contains(&status) {
                count(PURPOSE, "sent");
                return StepAttempt::Done(Box::new(OutboundDone {
                    detail: None,
                    output: json!({
                        "status": status,
                        "bytes": size,
                        "key": ctx.scrub.text(path.trim_start_matches('/')),
                        "attempts": attempt,
                    }),
                    data: None,
                }));
            }
            if is_retryable_status(status) && policy.has_attempts_left(attempt) {
                count(PURPOSE, "retry");
                return StepAttempt::Retry {
                    after: policy.delay_after(attempt),
                };
            }
            count(PURPOSE, "failed");
            let text = String::from_utf8_lossy(&response.body);
            let snippet = bound_text(&ctx.scrub.text(&text), ERROR_SNIPPET_BYTES);
            failed(
                ctx.scrub,
                FailureCode::Http,
                &format!("the bucket answered with status {status}: {snippet}"),
            )
        }
        Err(error) => {
            if is_retryable_error(&error) && policy.has_attempts_left(attempt) {
                count(PURPOSE, "retry");
                return StepAttempt::Retry {
                    after: policy.delay_after(attempt),
                };
            }
            count(PURPOSE, "failed");
            failed(
                ctx.scrub,
                FailureCode::Http,
                &format!("{} (attempt {attempt})", describe(&error)),
            )
        }
    }
}
