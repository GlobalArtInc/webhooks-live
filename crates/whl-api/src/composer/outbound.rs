use std::net::IpAddr;
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use whl_forward::{
    ForwardError, MAX_RESPONSE_HEADER_VALUE_BYTES, OutboundRequest, ResponseHeader,
    SELECTED_RESPONSE_HEADERS, TargetUrl,
};
use whl_store::Uuid;
use whl_telemetry::series;

use super::input::SendInput;
use super::settings::{Clients, LocalDelivery, LocalRequest, MAX_RESPONSE_BODY_BYTES};
use super::target::LocalTarget;

pub const HOP_HEADER: HeaderName = HeaderName::from_static("x-webhooks-live-hop");
pub const COMPOSER_HEADER: HeaderName = HeaderName::from_static("x-webhooks-live-composer");
const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
const PURPOSE: &str = "composer";

#[derive(Debug)]
pub struct Executed {
    pub status: Option<u16>,
    pub headers: Vec<ResponseHeader>,
    pub body: Bytes,
    pub truncated: bool,
    pub final_url: Option<String>,
    pub error: Option<String>,
    pub capture_id: Option<Uuid>,
    pub elapsed: Duration,
}

impl Executed {
    pub fn duration_ms(&self) -> u64 {
        u64::try_from(self.elapsed.as_millis()).unwrap_or(u64::MAX)
    }

    fn failed(error: String, elapsed: Duration, outcome: &'static str) -> Self {
        count(outcome);
        Self {
            status: None,
            headers: Vec::new(),
            body: Bytes::new(),
            truncated: false,
            final_url: None,
            error: Some(error),
            capture_id: None,
            elapsed,
        }
    }
}

fn count(outcome: &'static str) {
    metrics::counter!(series::OUTBOUND_REQUESTS_TOTAL, "purpose" => PURPOSE, "outcome" => outcome)
        .increment(1);
}

fn status_outcome(status: u16) -> &'static str {
    if status < 400 {
        "success"
    } else {
        "http_error"
    }
}

fn error_outcome(error: &ForwardError) -> &'static str {
    match error {
        ForwardError::TargetRefused(_) | ForwardError::RedirectRefused(_) => "blocked",
        ForwardError::ConnectTimeout | ForwardError::HopTimeout | ForwardError::TotalTimeout => {
            "timeout"
        }
        ForwardError::DnsFailure(_)
        | ForwardError::TooManyRedirects { .. }
        | ForwardError::Transport(_)
        | ForwardError::Configuration(_) => "connect_error",
    }
}

fn base_headers(input: &SendInput, hop: u32) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for header in &input.headers {
        headers.append(header.name.clone(), header.value.clone());
    }
    headers.insert(HOP_HEADER, HeaderValue::from(hop));
    headers
}

pub async fn run_external(
    clients: &Clients,
    input: &SendInput,
    target: TargetUrl,
    hop: u32,
) -> Executed {
    let request = OutboundRequest::new(
        input.method.clone(),
        target,
        base_headers(input, hop),
        input.body.clone(),
    );
    let client = if input.follow_redirects {
        &clients.following
    } else {
        &clients.direct
    };
    let started = Instant::now();
    match tokio::time::timeout(input.timeout, client.send(request)).await {
        Err(_) => Executed::failed(
            "timeout: the request took too long".to_owned(),
            started.elapsed(),
            "timeout",
        ),
        Ok(Err(error)) => Executed::failed(
            format!("{}: {error}", error.code()),
            started.elapsed(),
            error_outcome(&error),
        ),
        Ok(Ok(response)) => {
            let status = response.status.as_u16();
            count(status_outcome(status));
            Executed {
                status: Some(status),
                headers: response.headers,
                body: response.body,
                truncated: response.truncated,
                final_url: Some(response.final_url.unpack().to_string()),
                error: None,
                capture_id: None,
                elapsed: response.elapsed,
            }
        }
    }
}

pub async fn run_local(
    local: &dyn LocalDelivery,
    target: LocalTarget,
    input: &SendInput,
    client_ip: IpAddr,
    hop: u32,
) -> Executed {
    let mut headers = base_headers(input, hop);
    headers.insert(COMPOSER_HEADER, HeaderValue::from_static("1"));
    let request = LocalRequest {
        method: input.method.clone(),
        host: target.host,
        path_and_query: target.path_and_query,
        headers,
        body: input.body.clone(),
        client_ip,
    };
    let started = Instant::now();
    match tokio::time::timeout(input.timeout, local.deliver(request)).await {
        Err(_) => Executed::failed(
            "timeout: the request took too long".to_owned(),
            started.elapsed(),
            "timeout",
        ),
        Ok(Err(error)) => {
            tracing::error!(%error, "the in-process delivery of a composer request failed");
            Executed::failed(
                "local_delivery: the capture could not be handled".to_owned(),
                started.elapsed(),
                "connect_error",
            )
        }
        Ok(Ok(reply)) => {
            let status = reply.status.as_u16();
            count(status_outcome(status));
            let capture_id = reply
                .headers
                .get(REQUEST_ID_HEADER)
                .and_then(|value| value.to_str().ok())
                .and_then(|text| Uuid::try_parse(text).ok());
            let truncated = reply.body.len() > MAX_RESPONSE_BODY_BYTES;
            let body = if truncated {
                reply.body.slice(..MAX_RESPONSE_BODY_BYTES)
            } else {
                reply.body
            };
            Executed {
                status: Some(status),
                headers: select_headers(&reply.headers),
                body,
                truncated,
                final_url: Some(input.url.as_str().to_owned()),
                error: None,
                capture_id,
                elapsed: started.elapsed(),
            }
        }
    }
}

fn select_headers(headers: &HeaderMap) -> Vec<ResponseHeader> {
    SELECTED_RESPONSE_HEADERS
        .iter()
        .flat_map(|name| {
            headers.get_all(name).iter().map(|value| ResponseHeader {
                name: name.clone(),
                value: bounded(value),
            })
        })
        .collect()
}

fn bounded(value: &HeaderValue) -> String {
    let text = String::from_utf8_lossy(value.as_bytes());
    if text.len() <= MAX_RESPONSE_HEADER_VALUE_BYTES {
        return text.into_owned();
    }
    let end = (0..=MAX_RESPONSE_HEADER_VALUE_BYTES)
        .rev()
        .find(|index| text.is_char_boundary(*index))
        .unwrap_or(0);
    text[..end].to_owned()
}
