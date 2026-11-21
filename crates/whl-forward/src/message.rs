use std::time::Duration;

use bytes::Bytes;
use http::{HeaderMap, HeaderName, Method, StatusCode};

use crate::target::TargetUrl;

#[derive(Debug, Clone)]
pub struct OutboundRequest {
    pub method: Method,
    pub url: TargetUrl,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl OutboundRequest {
    pub fn new(method: Method, url: TargetUrl, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            method,
            url,
            headers,
            body,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseHeader {
    pub name: HeaderName,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct OutboundResponse {
    pub status: StatusCode,
    pub headers: Vec<ResponseHeader>,
    pub body: Bytes,
    pub truncated: bool,
    pub redirects_followed: u32,
    pub final_url: TargetUrl,
    pub elapsed: Duration,
}
