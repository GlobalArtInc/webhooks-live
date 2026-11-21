use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderName, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use whl_core::client_ip::TrustedProxies;
use whl_core::limiter::{Limiters, Verdict, whole_seconds_up};
use whl_telemetry::series;

use crate::error::ApiError;

const LIMIT_HEADER: HeaderName = HeaderName::from_static("ratelimit-limit");
const REMAINING_HEADER: HeaderName = HeaderName::from_static("ratelimit-remaining");
const CREATE_PATH: &str = "/api/v1/endpoints";
const SCOPE_API_IP: &str = "api_ip";
const SCOPE_CREATE_IP: &str = "create_ip";

#[derive(Debug, Clone)]
pub struct ApiLimits {
    limiters: Limiters,
    proxies: TrustedProxies,
}

impl ApiLimits {
    pub fn new(limiters: Limiters, proxies: TrustedProxies) -> Self {
        Self { limiters, proxies }
    }
}

struct Standing {
    limit: u32,
    remaining: u32,
}

pub async fn enforce_rate_limit(
    State(limits): State<ApiLimits>,
    request: Request,
    next: Next,
) -> Response {
    let Some(peer) = request.extensions().get::<ConnectInfo<SocketAddr>>() else {
        tracing::error!("the management API is served without connection info");
        return ApiError::ClientUnknown.into_response();
    };
    let client = limits
        .proxies
        .resolve(peer.0.ip(), request.headers())
        .client_ip;

    let standing = match limits.limiters.check_api_ip(client) {
        Verdict::Allowed { limit, remaining } => Standing { limit, remaining },
        Verdict::Limited { limit, retry_after } => {
            return refuse(SCOPE_API_IP, limit, whole_seconds_up(retry_after));
        }
    };
    if creates_endpoint(&request)
        && let Verdict::Limited { limit, retry_after } = limits.limiters.check_create_ip(client)
    {
        return refuse(SCOPE_CREATE_IP, limit, whole_seconds_up(retry_after));
    }

    let mut response = next.run(request).await;
    stamp(&mut response, &standing);
    response
}

fn creates_endpoint(request: &Request) -> bool {
    request.method() == Method::POST && request.uri().path() == CREATE_PATH
}

fn refuse(scope: &'static str, limit: u32, retry_after_secs: u64) -> Response {
    metrics::counter!(series::RATE_LIMITED_TOTAL, "scope" => scope).increment(1);
    let mut response = ApiError::RateLimited { retry_after_secs }.into_response();
    stamp(
        &mut response,
        &Standing {
            limit,
            remaining: 0,
        },
    );
    response
}

fn stamp(response: &mut Response, standing: &Standing) {
    let headers = response.headers_mut();
    headers.insert(LIMIT_HEADER, HeaderValue::from(standing.limit));
    headers.insert(REMAINING_HEADER, HeaderValue::from(standing.remaining));
}
