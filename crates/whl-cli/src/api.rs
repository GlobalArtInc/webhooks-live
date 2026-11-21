use std::collections::VecDeque;
use std::error::Error as _;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CACHE_CONTROL, HeaderValue, RETRY_AFTER};
use reqwest::{Client, Response, StatusCode, redirect};
use url::Url;

use crate::model::{CaptureDetail, CaptureId, CapturePage, EndpointId, ErrorEnvelope, Token};
use crate::output::{MAX_MESSAGE_CHARS, safe_to};
use crate::sse::{SseError, SseEvent, SseParser};

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;
pub const PAGE_LIMIT: u32 = 200;
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(300);
const LAST_EVENT_ID: &str = "last-event-id";
const CAPTURE_NOT_FOUND: &str = "capture_not_found";

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("the server refused the credentials (HTTP {status}): {message}")]
    Unauthorized { status: u16, message: String },
    #[error("the endpoint is gone or unknown: {message}")]
    Gone { message: String },
    #[error("the capture no longer exists")]
    CaptureNotFound,
    #[error("the server answered HTTP {status}: {message}")]
    Unavailable {
        status: u16,
        message: String,
        retry_after: Option<Duration>,
    },
    #[error("the server answered HTTP {status}: {message}")]
    Rejected { status: u16, message: String },
    #[error("network error: {0}")]
    Network(String),
    #[error("malformed answer from the server: {0}")]
    Malformed(String),
    #[error("the server sent nothing for {0} seconds")]
    Idle(u64),
    #[error("cannot build the HTTP client: {0}")]
    Client(String),
}

impl ApiError {
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized { .. } | Self::Gone { .. } | Self::Rejected { .. } | Self::Client(_)
        )
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Unavailable { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

impl From<SseError> for ApiError {
    fn from(error: SseError) -> Self {
        Self::Malformed(error.to_string())
    }
}

pub fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn network(error: reqwest::Error) -> ApiError {
    ApiError::Network(describe(&error.without_url()))
}

#[derive(Debug, Clone)]
pub struct ApiSettings {
    pub base: Url,
    pub endpoint: EndpointId,
    pub token: Option<Token>,
}

pub struct Api {
    client: Client,
    base: Url,
    endpoint: EndpointId,
    authorization: Option<HeaderValue>,
}

impl Api {
    pub fn new(settings: ApiSettings) -> Result<Self, ApiError> {
        let mut builder = Client::builder()
            .user_agent(concat!("webhooks-live-cli/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(redirect::Policy::none());
        if settings.base.scheme() == "http" {
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|error| ApiError::Client(describe(&error)))?;
        let authorization = match &settings.token {
            Some(token) => {
                let mut value = HeaderValue::from_str(&format!("Bearer {}", token.unpack()))
                    .map_err(|_| {
                        ApiError::Client("the token is not a valid header value".to_owned())
                    })?;
                value.set_sensitive(true);
                Some(value)
            }
            None => None,
        };
        Ok(Self {
            client,
            base: settings.base,
            endpoint: settings.endpoint,
            authorization,
        })
    }

    fn url(&self, tail: &[&str]) -> Url {
        let mut url = self.base.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            segments.clear();
            segments.extend(["api", "v1", "endpoints", self.endpoint.unpack()]);
            segments.extend(tail);
        }
        url
    }

    fn get(&self, url: Url) -> reqwest::RequestBuilder {
        let request = self.client.get(url);
        match &self.authorization {
            Some(value) => request.header(AUTHORIZATION, value.clone()),
            None => request,
        }
    }

    pub async fn open_stream(&self, last_event_id: Option<u64>) -> Result<EventStream, ApiError> {
        let mut request = self
            .get(self.url(&["stream"]))
            .header(ACCEPT, "text/event-stream")
            .header(CACHE_CONTROL, "no-store");
        if let Some(seq) = last_event_id {
            request = request.header(LAST_EVENT_ID, seq.to_string());
        }
        let response = tokio::time::timeout(REQUEST_TIMEOUT, request.send())
            .await
            .map_err(|_| ApiError::Network("timed out waiting for the stream to open".to_owned()))?
            .map_err(network)?;
        let response = ensure_success(response).await?;
        Ok(EventStream {
            response,
            parser: SseParser::new(),
            queue: VecDeque::new(),
        })
    }

    pub async fn list_after(
        &self,
        after_seq: u64,
        cursor: Option<&str>,
    ) -> Result<CapturePage, ApiError> {
        let mut url = self.url(&["captures"]);
        {
            let mut query = url.query_pairs_mut();
            query.append_pair("after_seq", &after_seq.to_string());
            query.append_pair("order", "asc");
            query.append_pair("limit", &PAGE_LIMIT.to_string());
            if let Some(cursor) = cursor {
                query.append_pair("cursor", cursor);
            }
        }
        let response = self
            .get(url)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(network)?;
        let response = ensure_success(response).await?;
        let bytes = read_limited(response, MAX_BODY_BYTES).await?;
        serde_json::from_slice(&bytes).map_err(|error| ApiError::Malformed(error.to_string()))
    }

    pub async fn detail(&self, id: &CaptureId) -> Result<CaptureDetail, ApiError> {
        let response = self
            .get(self.url(&["captures", id.unpack()]))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(network)?;
        let response = ensure_success(response).await?;
        let bytes = read_limited(response, MAX_BODY_BYTES).await?;
        serde_json::from_slice(&bytes).map_err(|error| ApiError::Malformed(error.to_string()))
    }

    pub async fn body(&self, id: &CaptureId) -> Result<Vec<u8>, ApiError> {
        let response = self
            .get(self.url(&["captures", id.unpack(), "body"]))
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(network)?;
        let response = ensure_success(response).await?;
        read_limited(response, MAX_BODY_BYTES).await
    }
}

async fn read_limited(mut response: Response, limit: u64) -> Result<Vec<u8>, ApiError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(ApiError::Malformed(format!(
            "the answer is larger than {limit} bytes"
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network)? {
        if bytes.len() as u64 + chunk.len() as u64 > limit {
            return Err(ApiError::Malformed(format!(
                "the answer is larger than {limit} bytes"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn retry_after(response: &Response) -> Option<Duration> {
    let seconds = response
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
}

async fn ensure_success(response: Response) -> Result<Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let retry_after = retry_after(&response);
    let body = tokio::time::timeout(
        ERROR_BODY_TIMEOUT,
        read_limited(response, MAX_ERROR_BODY_BYTES as u64),
    )
    .await
    .ok()
    .and_then(Result::ok)
    .unwrap_or_default();
    let (code, message) = match serde_json::from_slice::<ErrorEnvelope>(&body) {
        Ok(envelope) if !envelope.error.message.is_empty() => {
            (envelope.error.code, envelope.error.message)
        }
        Ok(envelope) => (envelope.error.code, status.to_string()),
        Err(_) => (String::new(), status.to_string()),
    };
    Err(classify(
        status,
        &code,
        server_message(&message),
        retry_after,
    ))
}

fn server_message(raw: &str) -> String {
    safe_to(raw, MAX_MESSAGE_CHARS)
}

fn classify(
    status: StatusCode,
    code: &str,
    message: String,
    retry_after: Option<Duration>,
) -> ApiError {
    let status_code = status.as_u16();
    match status_code {
        401 | 403 => ApiError::Unauthorized {
            status: status_code,
            message,
        },
        404 if code == CAPTURE_NOT_FOUND => ApiError::CaptureNotFound,
        404 | 410 => ApiError::Gone { message },
        408 | 429 | 500..=599 => ApiError::Unavailable {
            status: status_code,
            message,
            retry_after,
        },
        _ => ApiError::Rejected {
            status: status_code,
            message,
        },
    }
}

pub struct EventStream {
    response: Response,
    parser: SseParser,
    queue: VecDeque<SseEvent>,
}

impl EventStream {
    pub async fn next_event(&mut self) -> Result<Option<SseEvent>, ApiError> {
        loop {
            if let Some(event) = self.queue.pop_front() {
                return Ok(Some(event));
            }
            let chunk = tokio::time::timeout(STREAM_IDLE_TIMEOUT, self.response.chunk())
                .await
                .map_err(|_| ApiError::Idle(STREAM_IDLE_TIMEOUT.as_secs()))?
                .map_err(network)?;
            match chunk {
                Some(bytes) => self.queue.extend(self.parser.feed(&bytes)?),
                None => return Ok(None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treats_unauthorized_and_forbidden_as_fatal_credentials_errors() {
        for status in [401, 403] {
            let error = classify(
                StatusCode::from_u16(status).expect("valid status"),
                "invalid_token",
                "no".to_owned(),
                None,
            );
            assert!(matches!(error, ApiError::Unauthorized { .. }));
            assert!(error.is_fatal());
        }
    }

    #[test]
    fn treats_gone_as_fatal_and_a_missing_capture_as_skippable() {
        let gone = classify(StatusCode::GONE, "endpoint_gone", "gone".to_owned(), None);
        assert!(matches!(gone, ApiError::Gone { .. }));
        let missing = classify(
            StatusCode::NOT_FOUND,
            CAPTURE_NOT_FOUND,
            "missing".to_owned(),
            None,
        );
        assert!(matches!(missing, ApiError::CaptureNotFound));
        assert!(!missing.is_fatal());
    }

    #[test]
    fn retries_rate_limits_and_server_errors_with_the_advertised_delay() {
        let error = classify(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "slow".to_owned(),
            Some(Duration::from_secs(7)),
        );
        assert!(!error.is_fatal());
        assert_eq!(error.retry_after(), Some(Duration::from_secs(7)));
        let unavailable = classify(StatusCode::SERVICE_UNAVAILABLE, "", String::new(), None);
        assert!(!unavailable.is_fatal());
    }

    #[test]
    fn escapes_and_truncates_a_message_from_the_server() {
        let shown = server_message(&format!("\u{1b}]0;owned\u{7}{}", "x".repeat(2000)));
        assert!(!shown.contains('\u{1b}'));
        assert!(!shown.contains('\u{7}'));
        assert!(shown.ends_with('…'));
        assert!(shown.chars().count() < 600);
    }

    #[test]
    fn refuses_other_client_errors_as_fatal() {
        let error = classify(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "bad".to_owned(),
            None,
        );
        assert!(error.is_fatal());
    }
}
