use std::collections::HashSet;

use http::Method;
use http::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use thiserror::Error;
use url::Url;
use whl_store::forward::{ClaimedJob, JobCapture};

use crate::config::GuardConfig;
use crate::error::ForwardError;
use crate::message::OutboundRequest;
use crate::target::validate_target_url;

pub const CAPTURE_ID_HEADER: &str = "x-webhooks-live-capture-id";
pub const HOP_HEADER: &str = "x-webhooks-live-hop";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RequestError {
    #[error(transparent)]
    Target(#[from] ForwardError),
    #[error("the job method is not a valid HTTP method")]
    Method,
}

pub fn build_request(
    job: &ClaimedJob,
    capture: &JobCapture,
    config: &GuardConfig,
) -> Result<OutboundRequest, RequestError> {
    let method = Method::from_bytes(job.method.as_bytes()).map_err(|_| RequestError::Method)?;
    let target = validate_target_url(&job.target_url, config)?;
    let url = if job.append_path {
        let appended = append_capture_path(target.unpack(), &capture.path, &capture.query_string);
        validate_target_url(appended.as_str(), config)?
    } else {
        target
    };
    let body = job.payload.clone().unwrap_or_else(|| capture.body.clone());
    Ok(OutboundRequest::new(
        method,
        url,
        headers(job, capture),
        body,
    ))
}

fn outgoing_hop(capture: &JobCapture) -> u32 {
    let incoming = capture
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(HOP_HEADER))
        .map(|header| header.value.trim().parse::<u32>().unwrap_or(u32::MAX))
        .max()
        .unwrap_or(0);
    incoming.saturating_add(1)
}

fn headers(job: &ClaimedJob, capture: &JobCapture) -> HeaderMap {
    let mut map = HeaderMap::new();
    let allowed: HashSet<String> = job
        .header_allowlist
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    for header in &capture.headers {
        let lowered = header.name.to_ascii_lowercase();
        if !allowed.contains(&lowered)
            || lowered == CONTENT_TYPE.as_str()
            || lowered == CAPTURE_ID_HEADER
            || lowered == HOP_HEADER
        {
            continue;
        }
        let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(lowered.as_bytes()),
            HeaderValue::from_str(&header.value),
        ) else {
            continue;
        };
        map.append(name, value);
    }
    let content_type = job
        .content_type
        .as_deref()
        .or(capture.content_type.as_deref())
        .and_then(|text| HeaderValue::from_str(text).ok());
    if let Some(value) = content_type {
        map.insert(CONTENT_TYPE, value);
    }
    if let Ok(value) = HeaderValue::from_str(&job.capture_id.hyphenated().to_string()) {
        map.insert(HeaderName::from_static(CAPTURE_ID_HEADER), value);
    }
    map.insert(
        HeaderName::from_static(HOP_HEADER),
        HeaderValue::from(outgoing_hop(capture)),
    );
    map
}

fn append_capture_path(base: &Url, path: &str, query_string: &str) -> Url {
    let mut url = base.clone();
    let prefix = url.path().trim_end_matches('/').to_owned();
    url.set_path(&format!("{prefix}{}", clean_sub_path(path)));
    if !query_string.is_empty() {
        let merged = match url.query() {
            Some(existing) if !existing.is_empty() => format!("{existing}&{query_string}"),
            _ => query_string.to_owned(),
        };
        url.set_query(Some(&merged));
    }
    url
}

fn clean_sub_path(path: &str) -> String {
    let mut cleaned = String::new();
    for segment in path.split(['/', '\\']).skip(1) {
        if is_dot_segment(segment) {
            continue;
        }
        cleaned.push('/');
        cleaned.push_str(segment);
    }
    cleaned
}

fn is_dot_segment(segment: &str) -> bool {
    let decoded = segment.replace("%2e", ".").replace("%2E", ".");
    decoded == "." || decoded == ".."
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use whl_store::Uuid;
    use whl_store::captures::NameValue;
    use whl_store::forward::JobKind;

    use super::*;

    fn job(target: &str) -> ClaimedJob {
        ClaimedJob {
            id: Uuid::nil(),
            capture_id: Uuid::nil(),
            rule_id: None,
            kind: JobKind::Rule,
            target_url: target.to_owned(),
            method: "POST".to_owned(),
            content_type: None,
            header_allowlist: Vec::new(),
            append_path: false,
            attempt: 1,
            payload: None,
        }
    }

    fn capture(headers: &[(&str, &str)]) -> JobCapture {
        JobCapture {
            endpoint_id: "e".to_owned(),
            path: "/orders/42".to_owned(),
            query_string: "a=1".to_owned(),
            content_type: Some("application/json".to_owned()),
            headers: headers
                .iter()
                .map(|(name, value)| NameValue {
                    name: (*name).to_owned(),
                    value: (*value).to_owned(),
                })
                .collect(),
            body: Bytes::from_static(b"{\"x\":1}"),
        }
    }

    fn config() -> GuardConfig {
        GuardConfig::default()
    }

    #[test]
    fn copies_only_allow_listed_headers_and_the_fixed_set() {
        let mut claimed = job("https://hooks.example.com/in");
        claimed.header_allowlist = vec!["X-Signature".to_owned(), "content-type".to_owned()];
        let source = capture(&[
            ("x-signature", "abc"),
            ("X-Signature", "def"),
            ("Authorization", "secret"),
            ("Cookie", "s=1"),
            ("Content-Type", "text/evil"),
            ("X-Webhooks-Live-Capture-Id", "spoofed"),
        ]);
        let request = build_request(&claimed, &source, &config()).unwrap();
        let signatures: Vec<&str> = request
            .headers
            .get_all("x-signature")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(signatures, ["abc", "def"]);
        assert!(request.headers.get("authorization").is_none());
        assert!(request.headers.get("cookie").is_none());
        assert_eq!(
            request.headers.get("content-type").unwrap(),
            "application/json"
        );
        assert_eq!(
            request.headers.get(CAPTURE_ID_HEADER).unwrap(),
            "00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(request.headers.get(HOP_HEADER).unwrap(), "1");
        assert_eq!(request.headers.len(), 5);
        assert_eq!(request.body, Bytes::from_static(b"{\"x\":1}"));
    }

    #[test]
    fn applies_method_and_content_type_overrides() {
        let mut claimed = job("https://hooks.example.com/in");
        claimed.method = "PUT".to_owned();
        claimed.content_type = Some("text/plain".to_owned());
        let request = build_request(&claimed, &capture(&[]), &config()).unwrap();
        assert_eq!(request.method, Method::PUT);
        assert_eq!(request.headers.get("content-type").unwrap(), "text/plain");
    }

    #[test]
    fn appends_the_captured_path_and_query_when_asked() {
        let mut claimed = job("https://hooks.example.com/in/?token=t");
        claimed.append_path = true;
        let request = build_request(&claimed, &capture(&[]), &config()).unwrap();
        assert_eq!(
            request.url.unpack().as_str(),
            "https://hooks.example.com/in/orders/42?token=t&a=1"
        );
    }

    #[test]
    fn drops_dot_segments_from_the_appended_path() {
        let mut claimed = job("https://hooks.example.com/in");
        claimed.append_path = true;
        for hostile in ["/../admin", "/%2e%2E/admin", "/a/..\\admin", "/./../admin"] {
            let mut source = capture(&[]);
            source.path = hostile.to_owned();
            source.query_string = String::new();
            let request = build_request(&claimed, &source, &config()).unwrap();
            assert!(
                request.url.unpack().path().starts_with("/in/"),
                "{hostile} gave {}",
                request.url.unpack()
            );
            assert!(!request.url.unpack().path().contains(".."), "{hostile}");
        }
    }

    #[test]
    fn sends_the_incoming_hop_plus_one_even_when_the_header_is_allow_listed() {
        let mut claimed = job("https://hooks.example.com/in");
        claimed.header_allowlist = vec!["X-Webhooks-Live-Hop".to_owned()];
        let request = build_request(
            &claimed,
            &capture(&[("X-Webhooks-Live-Hop", "2")]),
            &config(),
        )
        .unwrap();
        let hops: Vec<&str> = request
            .headers
            .get_all(HOP_HEADER)
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(hops, ["3"]);
    }

    #[test]
    fn sends_the_payload_instead_of_the_captured_body_when_the_job_has_one() {
        let mut claimed = job("https://hooks.example.com/in");
        claimed.payload = Some(Bytes::from_static(b"shaped"));
        let request = build_request(&claimed, &capture(&[]), &config()).unwrap();
        assert_eq!(request.body, Bytes::from_static(b"shaped"));
    }

    #[test]
    fn refuses_a_refused_target_and_a_bad_method() {
        let error =
            build_request(&job("http://169.254.169.254/"), &capture(&[]), &config()).unwrap_err();
        assert!(matches!(error, RequestError::Target(_)));
        let mut claimed = job("https://hooks.example.com/");
        claimed.method = "NOT A METHOD".to_owned();
        assert_eq!(
            build_request(&claimed, &capture(&[]), &config()).unwrap_err(),
            RequestError::Method
        );
    }
}
