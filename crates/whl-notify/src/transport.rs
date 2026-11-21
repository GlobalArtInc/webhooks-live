use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use http::{HeaderMap, Method};
use url::Url;
use whl_forward::{
    DEFAULT_USER_AGENT, ForwardError, GuardConfig, GuardedClient, LookupHost, OutboundRequest,
    SystemLookup, validate_target_url,
};

use crate::adapter::{Destination, MailPlan, Prepared};
use crate::mail::{MailError, Mailer, OutgoingMail};
use crate::vendor::{PinError, VendorHosts};

pub const REPLY_BODY_BYTES: usize = 4096;
pub const DELIVERY_HEADER: &str = "x-webhooks-live-delivery";
pub const HOP_HEADER: &str = "x-webhooks-live-hop";
const HOP_VALUE: &str = "1";
const SMTP_ACCEPTED: u16 = 250;

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TRANSPORT_DETAIL_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("refused_target: {0}")]
    Refused(String),
    #[error("not_pinned: {0}")]
    NotPinned(PinError),
    #[error("transport: {0}")]
    Transient(String),
    #[error("{0}")]
    Final(String),
    #[error("configuration: {0}")]
    Configuration(String),
}

impl SendError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Bytes,
}

#[derive(Debug, Clone)]
pub struct Transport {
    webhook: GuardedClient,
    vendor: VendorRoute,
    hosts: VendorHosts,
    guard: GuardConfig,
    mailer: Option<Mailer>,
}

#[derive(Debug, Clone)]
enum VendorRoute {
    Direct(GuardedClient),
    Proxy(reqwest::Client),
}

impl Transport {
    pub fn new(
        guard: GuardConfig,
        proxy: Option<&Url>,
        hosts: VendorHosts,
    ) -> Result<Self, SendError> {
        Self::with_lookup(guard, proxy, hosts, Arc::new(SystemLookup))
    }

    pub fn with_lookup(
        mut guard: GuardConfig,
        proxy: Option<&Url>,
        hosts: VendorHosts,
        lookup: Arc<dyn LookupHost>,
    ) -> Result<Self, SendError> {
        guard.max_response_bytes = REPLY_BODY_BYTES;
        let configuration = |error: ForwardError| SendError::Configuration(error.to_string());
        let webhook = GuardedClient::with_lookup(guard.clone(), Arc::clone(&lookup))
            .map_err(configuration)?;
        let vendor = match proxy {
            Some(proxy) => VendorRoute::Proxy(proxy_client(proxy)?),
            None => {
                let mut direct = guard.clone();
                direct.max_redirects = 0;
                VendorRoute::Direct(
                    GuardedClient::with_lookup(direct, lookup).map_err(configuration)?,
                )
            }
        };
        Ok(Self {
            webhook,
            vendor,
            hosts,
            guard,
            mailer: None,
        })
    }

    pub fn with_mailer(mut self, mailer: Mailer) -> Self {
        self.mailer = Some(mailer);
        self
    }

    pub fn attempt_deadline(&self) -> Duration {
        self.guard.total_timeout.max(ATTEMPT_TIMEOUT) + Duration::from_secs(1)
    }

    pub async fn send(&self, prepared: &Prepared) -> Result<Reply, SendError> {
        let work = self.dispatch(prepared);
        tokio::time::timeout(self.attempt_deadline(), work)
            .await
            .unwrap_or_else(|_| Err(SendError::Transient("timeout".to_owned())))
    }

    async fn dispatch(&self, prepared: &Prepared) -> Result<Reply, SendError> {
        match &prepared.destination {
            Destination::Guarded(url) => self.send_guarded(&self.webhook, url, prepared).await,
            Destination::Vendor(url) => {
                let resolved = self.hosts.resolve(url).map_err(SendError::NotPinned)?;
                match &self.vendor {
                    VendorRoute::Direct(client) => {
                        self.send_guarded(client, resolved.as_str(), prepared).await
                    }
                    VendorRoute::Proxy(client) => send_proxied(client, resolved, prepared).await,
                }
            }
            Destination::Mail(plan) => self.send_mail(plan, prepared).await,
        }
    }

    async fn send_mail(&self, plan: &MailPlan, prepared: &Prepared) -> Result<Reply, SendError> {
        let mailer = self.mailer.as_ref().ok_or_else(|| {
            SendError::Configuration("email notifications are not configured".to_owned())
        })?;
        let body = String::from_utf8(prepared.body.clone())
            .map_err(|_| SendError::Final("invalid_message: the body is not text".to_owned()))?;
        let mail = OutgoingMail {
            to: plan.to.clone(),
            subject: plan.subject.clone(),
            body,
            unsubscribe: Some(plan.unsubscribe.clone()),
        };
        match mailer.send(&mail).await {
            Ok(()) => Ok(Reply {
                status: SMTP_ACCEPTED,
                body: Bytes::new(),
            }),
            Err(error) if error.is_retryable() => Err(SendError::Transient(error.to_string())),
            Err(error) => Err(SendError::Final(mail_failure(&error))),
        }
    }

    async fn send_guarded(
        &self,
        client: &GuardedClient,
        url: &str,
        prepared: &Prepared,
    ) -> Result<Reply, SendError> {
        let target = validate_target_url(url, &self.guard).map_err(classify_forward)?;
        let mut headers = json_headers();
        if let Some(delivery) = &prepared.delivery_header
            && let Ok(value) = HeaderValue::from_str(delivery)
        {
            headers.insert(HeaderName::from_static(DELIVERY_HEADER), value);
        }
        let request = OutboundRequest::new(
            Method::POST,
            target,
            headers,
            Bytes::from(prepared.body.clone()),
        );
        match client.send(request).await {
            Ok(response) => Ok(Reply {
                status: response.status.as_u16(),
                body: response.body,
            }),
            Err(error) => Err(classify_forward(error)),
        }
    }
}

fn mail_failure(error: &MailError) -> String {
    format!("mail_refused: {error}")
}

fn json_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        HeaderName::from_static(HOP_HEADER),
        HeaderValue::from_static(HOP_VALUE),
    );
    headers
}

fn classify_forward(error: ForwardError) -> SendError {
    match error {
        ForwardError::TargetRefused(_) | ForwardError::RedirectRefused(_) => {
            SendError::Refused(error.code().to_owned())
        }
        ForwardError::Configuration(detail) => SendError::Configuration(detail),
        ForwardError::TooManyRedirects { .. } => SendError::Final(error.code().to_owned()),
        other => SendError::Transient(other.code().to_owned()),
    }
}

fn proxy_client(proxy: &Url) -> Result<reqwest::Client, SendError> {
    let configuration =
        |error: reqwest::Error| SendError::Configuration(error.without_url().to_string());
    let route = reqwest::Proxy::all(proxy.as_str()).map_err(configuration)?;
    reqwest::Client::builder()
        .tls_backend_rustls()
        .proxy(route)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(ATTEMPT_TIMEOUT)
        .pool_max_idle_per_host(0)
        .user_agent(DEFAULT_USER_AGENT)
        .build()
        .map_err(configuration)
}

async fn send_proxied(
    client: &reqwest::Client,
    url: Url,
    prepared: &Prepared,
) -> Result<Reply, SendError> {
    let transient = |error: reqwest::Error| {
        let text: String = error
            .without_url()
            .to_string()
            .chars()
            .take(MAX_TRANSPORT_DETAIL_CHARS)
            .collect();
        SendError::Transient(text)
    };
    let mut response = client
        .post(url)
        .header(CONTENT_TYPE, "application/json")
        .header(HOP_HEADER, HOP_VALUE)
        .body(prepared.body.clone())
        .send()
        .await
        .map_err(transient)?;
    let status = response.status().as_u16();
    let mut body: Vec<u8> = Vec::new();
    while body.len() < REPLY_BODY_BYTES {
        let Some(chunk) = response.chunk().await.map_err(transient)? else {
            break;
        };
        let room = REPLY_BODY_BYTES - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
    }
    Ok(Reply {
        status,
        body: Bytes::from(body),
    })
}
