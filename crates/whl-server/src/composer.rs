use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::header::HOST;
use axum::http::{HeaderValue, Request};
use tower::ServiceExt;
use whl_api::{LocalDelivery, LocalError, LocalFuture, LocalReply, LocalRequest};

pub const MAX_LOCAL_REPLY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct CaptureDelivery {
    router: Router,
}

impl CaptureDelivery {
    pub fn new(router: Router) -> Self {
        Self { router }
    }
}

impl LocalDelivery for CaptureDelivery {
    fn deliver(&self, request: LocalRequest) -> LocalFuture<'_> {
        let router = self.router.clone();
        Box::pin(async move {
            let host = HeaderValue::from_str(&request.host)
                .map_err(|error| LocalError::Failed(error.to_string()))?;
            let mut http_request = Request::builder()
                .method(request.method)
                .uri(request.path_and_query)
                .body(Body::from(request.body))
                .map_err(|error| LocalError::Failed(error.to_string()))?;
            *http_request.headers_mut() = request.headers;
            http_request.headers_mut().insert(HOST, host);
            http_request
                .extensions_mut()
                .insert(ConnectInfo(SocketAddr::new(request.client_ip, 0)));
            let Ok(response) = router.oneshot(http_request).await;
            let (parts, body) = response.into_parts();
            let bytes = axum::body::to_bytes(body, MAX_LOCAL_REPLY_BYTES)
                .await
                .map_err(|error| LocalError::Failed(error.to_string()))?;
            Ok(LocalReply {
                status: parts.status,
                headers: parts.headers,
                body: bytes,
            })
        })
    }
}
