mod client;
mod config;
mod error;
mod lookup;
mod message;
mod outcome;
mod ranges;
mod redirect;
mod request;
mod resolver;
mod retry;
#[cfg(any(test, feature = "test-support"))]
mod static_lookup;
mod target;
mod vet;
mod worker;

pub use client::{
    GuardedClient, MAX_RESPONSE_HEADER_VALUE_BYTES, MAX_TRANSPORT_MESSAGE_BYTES,
    SELECTED_RESPONSE_HEADERS,
};
pub use config::{
    DEFAULT_MAX_REDIRECTS, DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_USER_AGENT, GuardConfig,
};
pub use error::{ForwardError, RefusalReason};
pub use lookup::{LookupFuture, LookupHost, SystemLookup};
pub use message::{OutboundRequest, OutboundResponse, ResponseHeader};
pub use outcome::{Outcome, RESPONSE_BODY_BYTES, Verdict};
pub use ranges::{
    Embedding, GLOBAL_UNICAST_IPV6, OUTSIDE_GLOBAL_UNICAST_NAME, REFUSED_IPV4_RANGES,
    REFUSED_IPV6_RANGES, RefusedIpv4Range, RefusedIpv6Range, Treatment, is_refused, refusing_range,
};
pub use request::{CAPTURE_ID_HEADER, RequestError, build_request};
pub use resolver::MAX_VETTED_ADDRESSES;
pub use retry::{
    DEFAULT_BACKOFF_MULTIPLIER, DEFAULT_INITIAL_BACKOFF, DEFAULT_JITTER_PERCENT,
    DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_BACKOFF, RetryPolicy,
};
#[cfg(any(test, feature = "test-support"))]
pub use static_lookup::StaticLookup;
pub use target::{MAX_URL_BYTES, TargetUrl, validate_target_url};
pub use vet::{TargetCheckError, vet_target};
pub use worker::{
    DEFAULT_LEASE, DEFAULT_POLL_INTERVAL, ForwardWorker, MAX_ATTEMPTS_LIMIT, MAX_CLAIM_BATCH,
    MIN_LEASE_MARGIN, WorkerConfig, WorkerError, WorkerHandle,
};
