use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use http::header::{CONTENT_LENGTH, CONTENT_TYPE, LOCATION};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use ipnet::IpNet;

use crate::config::GuardConfig;
use crate::error::{ForwardError, RefusalReason};
use crate::lookup::{LookupHost, SystemLookup};
use crate::message::{OutboundRequest, OutboundResponse, ResponseHeader};
use crate::ranges::is_refused;
use crate::redirect::{is_redirect_status, plan_redirect, strip_managed_headers};
use crate::resolver::{GuardedResolver, ResolveError};
use crate::target::{TargetUrl, check_url};

pub const SELECTED_RESPONSE_HEADERS: [HeaderName; 3] = [CONTENT_TYPE, CONTENT_LENGTH, LOCATION];
pub const MAX_RESPONSE_HEADER_VALUE_BYTES: usize = 2048;
pub const MAX_TRANSPORT_MESSAGE_BYTES: usize = 512;
const READ_BUFFER_HINT_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct GuardedClient {
    client: reqwest::Client,
    config: GuardConfig,
}

enum HopOutcome {
    Final(FinalResponse),
    Redirect {
        status: StatusCode,
        location: HeaderValue,
    },
}

struct FinalResponse {
    status: StatusCode,
    headers: Vec<ResponseHeader>,
    body: Bytes,
    truncated: bool,
}

impl GuardedClient {
    pub fn new(config: GuardConfig) -> Result<Self, ForwardError> {
        Self::with_lookup(config, Arc::new(SystemLookup))
    }

    pub fn with_lookup(
        config: GuardConfig,
        lookup: Arc<dyn LookupHost>,
    ) -> Result<Self, ForwardError> {
        config.validate()?;
        let resolver = GuardedResolver::new(lookup, &config.allow);
        let client = reqwest::Client::builder()
            .tls_backend_rustls()
            .dns_resolver(resolver)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(config.connect_timeout)
            .pool_max_idle_per_host(0)
            .user_agent(config.user_agent.as_str())
            .build()
            .map_err(|error| ForwardError::Configuration(describe(&error)))?;
        Ok(Self { client, config })
    }

    pub fn config(&self) -> &GuardConfig {
        &self.config
    }

    pub async fn send(&self, request: OutboundRequest) -> Result<OutboundResponse, ForwardError> {
        let started = Instant::now();
        let mut current = self.prepare(request)?;
        let mut followed: u32 = 0;
        loop {
            let remaining = self.config.total_timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(ForwardError::TotalTimeout);
            }
            let hop_budget_binds = self.config.hop_timeout <= remaining;
            let budget = remaining.min(self.config.hop_timeout);
            let outcome = tokio::time::timeout(budget, self.hop(&current))
                .await
                .map_err(|_| {
                    if hop_budget_binds {
                        ForwardError::HopTimeout
                    } else {
                        ForwardError::TotalTimeout
                    }
                })?
                .map_err(|error| error.at_hop(followed))?;
            match outcome {
                HopOutcome::Final(finished) => {
                    return Ok(OutboundResponse {
                        status: finished.status,
                        headers: finished.headers,
                        body: finished.body,
                        truncated: finished.truncated,
                        redirects_followed: followed,
                        final_url: current.url,
                        elapsed: started.elapsed(),
                    });
                }
                HopOutcome::Redirect { status, location } => {
                    if followed >= self.config.max_redirects {
                        return Err(ForwardError::TooManyRedirects {
                            limit: self.config.max_redirects,
                        });
                    }
                    current = plan_redirect(
                        current,
                        status,
                        &location,
                        &self.config.allow,
                        &self.config.hardening,
                    )
                    .map_err(ForwardError::RedirectRefused)?;
                    followed += 1;
                }
            }
        }
    }

    fn prepare(&self, mut request: OutboundRequest) -> Result<OutboundRequest, ForwardError> {
        let checked = check_url(
            request.url.unpack().clone(),
            &self.config.allow,
            &self.config.hardening,
        )
        .map_err(ForwardError::TargetRefused)?;
        request.url = TargetUrl::from_checked(checked);
        strip_managed_headers(&mut request.headers);
        Ok(request)
    }

    async fn hop(&self, request: &OutboundRequest) -> Result<HopOutcome, ForwardError> {
        let mut builder = self
            .client
            .request(request.method.clone(), request.url.unpack().clone())
            .headers(request.headers.clone());
        if !request.body.is_empty() {
            builder = builder.body(request.body.clone());
        }
        let mut response = builder.send().await.map_err(classify_transport)?;
        check_peer(response.remote_addr(), &self.config.allow)?;

        let status = response.status();
        if self.config.max_redirects > 0
            && is_redirect_status(status)
            && let Some(location) = response.headers().get(LOCATION)
        {
            return Ok(HopOutcome::Redirect {
                status,
                location: location.clone(),
            });
        }
        let headers = select_headers(response.headers());
        let (body, truncated) = read_capped(&mut response, self.config.max_response_bytes).await?;
        Ok(HopOutcome::Final(FinalResponse {
            status,
            headers,
            body,
            truncated,
        }))
    }
}

fn check_peer(peer: Option<SocketAddr>, allow: &[IpNet]) -> Result<(), ForwardError> {
    match peer {
        Some(peer) if !is_refused(peer.ip(), allow) => Ok(()),
        _ => Err(ForwardError::TargetRefused(RefusalReason::RefusedAddress)),
    }
}

async fn read_capped(
    response: &mut reqwest::Response,
    cap: usize,
) -> Result<(Bytes, bool), ForwardError> {
    let mut body: Vec<u8> = Vec::with_capacity(cap.min(READ_BUFFER_HINT_BYTES));
    loop {
        let Some(chunk) = response.chunk().await.map_err(classify_transport)? else {
            return Ok((Bytes::from(body), false));
        };
        let room = cap.saturating_sub(body.len());
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            return Ok((Bytes::from(body), true));
        }
        body.extend_from_slice(&chunk);
    }
}

fn select_headers(headers: &HeaderMap) -> Vec<ResponseHeader> {
    SELECTED_RESPONSE_HEADERS
        .iter()
        .flat_map(|name| {
            headers.get_all(name).iter().map(|value| ResponseHeader {
                name: name.clone(),
                value: bounded_value(value),
            })
        })
        .collect()
}

fn bounded_value(value: &HeaderValue) -> String {
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

fn find_resolve_error<'a>(error: &'a (dyn Error + 'static)) -> Option<&'a ResolveError> {
    let mut current = Some(error);
    while let Some(candidate) = current {
        if let Some(resolve) = candidate.downcast_ref::<ResolveError>() {
            return Some(resolve);
        }
        current = candidate.source();
    }
    None
}

fn describe(error: &(dyn Error + 'static)) -> String {
    let mut parts = Vec::new();
    let mut current = Some(error);
    while let Some(candidate) = current {
        parts.push(candidate.to_string());
        current = candidate.source();
    }
    let mut message = parts.join(": ");
    let end = (0..=MAX_TRANSPORT_MESSAGE_BYTES.min(message.len()))
        .rev()
        .find(|index| message.is_char_boundary(*index))
        .unwrap_or(0);
    message.truncate(end);
    message
}

fn classify_transport(error: reqwest::Error) -> ForwardError {
    let error = error.without_url();
    match find_resolve_error(&error) {
        Some(ResolveError::Refused) => {
            return ForwardError::TargetRefused(RefusalReason::RefusedAddress);
        }
        Some(ResolveError::NoAddresses) => {
            return ForwardError::DnsFailure("host has no addresses".to_owned());
        }
        Some(ResolveError::Lookup(message)) => return ForwardError::DnsFailure(message.clone()),
        None => {}
    }
    if error.is_timeout() && error.is_connect() {
        return ForwardError::ConnectTimeout;
    }
    ForwardError::Transport(describe(&error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treats_a_missing_or_refused_peer_address_as_refused() {
        let allow = ["127.0.0.1/32".parse().unwrap()];
        let refused = Err(ForwardError::TargetRefused(RefusalReason::RefusedAddress));
        assert_eq!(check_peer(None, &allow), refused);
        assert_eq!(
            check_peer(Some("10.0.0.1:80".parse().unwrap()), &allow),
            refused
        );
        assert_eq!(
            check_peer(Some("[::1]:80".parse().unwrap()), &allow),
            refused
        );
        assert_eq!(
            check_peer(Some("8.8.8.8:443".parse().unwrap()), &allow),
            Ok(())
        );
        assert_eq!(
            check_peer(Some("127.0.0.1:80".parse().unwrap()), &allow),
            Ok(())
        );
    }
}
