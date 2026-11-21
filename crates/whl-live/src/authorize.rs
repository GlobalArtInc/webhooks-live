use std::fmt;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;

use axum::http::HeaderMap;
use whl_core::endpoint_id::EndpointId;

#[derive(Clone)]
pub struct StreamRequest {
    headers: HeaderMap,
    peer: Option<IpAddr>,
}

impl StreamRequest {
    pub fn new(headers: HeaderMap, peer: Option<IpAddr>) -> Self {
        Self { headers, peer }
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn peer(&self) -> Option<IpAddr> {
        self.peer
    }
}

impl fmt::Debug for StreamRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamRequest")
            .field("peer", &self.peer)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamVerdict {
    Allowed,
    Denied,
    Unavailable,
}

pub type VerdictFuture<'a> = Pin<Box<dyn Future<Output = StreamVerdict> + Send + 'a>>;

pub trait StreamAuthorizer: fmt::Debug + Send + Sync + 'static {
    fn still_allowed<'a>(
        &'a self,
        endpoint: &'a EndpointId,
        request: &'a StreamRequest,
    ) -> VerdictFuture<'a>;

    fn still_allowed_all<'a>(
        &'a self,
        endpoints: &'a [EndpointId],
        request: &'a StreamRequest,
    ) -> VerdictFuture<'a> {
        Box::pin(async move {
            let mut unavailable = false;
            for endpoint in endpoints {
                match self.still_allowed(endpoint, request).await {
                    StreamVerdict::Allowed => {}
                    StreamVerdict::Denied => return StreamVerdict::Denied,
                    StreamVerdict::Unavailable => unavailable = true,
                }
            }
            if unavailable {
                StreamVerdict::Unavailable
            } else {
                StreamVerdict::Allowed
            }
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAll;

impl StreamAuthorizer for AllowAll {
    fn still_allowed<'a>(
        &'a self,
        _endpoint: &'a EndpointId,
        _request: &'a StreamRequest,
    ) -> VerdictFuture<'a> {
        Box::pin(std::future::ready(StreamVerdict::Allowed))
    }
}
