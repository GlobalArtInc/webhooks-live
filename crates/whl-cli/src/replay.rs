use std::time::{Duration, Instant};

use reqwest::header::{CONNECTION, HOST, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method, redirect};
use url::Url;

use crate::api::{CONNECT_TIMEOUT, describe};
use crate::model::CaptureDetail;

pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);

const STRIPPED_HEADERS: [&str; 12] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "expect",
    "host",
    "content-length",
];

#[derive(Debug, Clone, Default)]
pub struct HeaderRules {
    pub keep_host: bool,
    pub set: Vec<(HeaderName, HeaderValue)>,
    pub remove: Vec<HeaderName>,
}

#[derive(Debug, Clone)]
pub struct ReplayConfig {
    pub target: Url,
    pub strip_prefix: Option<String>,
    pub headers: HeaderRules,
}

#[derive(Debug)]
pub struct Plan {
    pub method: Method,
    pub url: Url,
    pub headers: HeaderMap,
    pub dropped: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error("the captured method {0:?} is not a valid HTTP method")]
    Method(String),
    #[error("the captured path would leave the target prefix {prefix}")]
    PathEscapesTarget { prefix: String },
    #[error("cannot build the HTTP client: {0}")]
    Client(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Delivered { status: u16 },
    Failed { reason: String },
}

#[derive(Debug, Clone)]
pub struct Sent {
    pub outcome: Outcome,
    pub elapsed: Duration,
}

pub struct Replayer {
    client: Client,
    config: ReplayConfig,
}

impl Replayer {
    pub fn new(config: ReplayConfig) -> Result<Self, ReplayError> {
        let client = Client::builder()
            .user_agent(concat!("webhooks-live-cli/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(CONNECT_TIMEOUT)
            .redirect(redirect::Policy::none())
            .no_proxy()
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|error| ReplayError::Client(describe(&error)))?;
        Ok(Self { client, config })
    }

    pub fn config(&self) -> &ReplayConfig {
        &self.config
    }

    pub fn plan(&self, detail: &CaptureDetail) -> Result<Plan, ReplayError> {
        let method = Method::from_bytes(detail.method.as_bytes())
            .map_err(|_| ReplayError::Method(detail.method.clone()))?;
        let url = self.build_url(&detail.path, &detail.query_string)?;
        let (mut headers, dropped) = captured_headers(detail);
        let rules = &self.config.headers;
        if rules.keep_host
            && let Ok(host) = HeaderValue::from_str(&detail.host)
            && !detail.host.is_empty()
        {
            headers.insert(HOST, host);
        }
        for name in &rules.remove {
            headers.remove(name);
        }
        for (name, value) in &rules.set {
            headers.insert(name.clone(), value.clone());
        }
        Ok(Plan {
            method,
            url,
            headers,
            dropped,
        })
    }

    fn build_url(&self, path: &str, query: &str) -> Result<Url, ReplayError> {
        let sub_path = strip_prefix(path, self.config.strip_prefix.as_deref());
        let base = self.config.target.path().trim_end_matches('/');
        let mut url = self.config.target.clone();
        url.set_path(&format!("{base}{sub_path}"));
        url.set_query((!query.is_empty()).then_some(query));
        let inside =
            base.is_empty() || url.path() == base || url.path().starts_with(&format!("{base}/"));
        if inside {
            Ok(url)
        } else {
            Err(ReplayError::PathEscapesTarget {
                prefix: base.to_owned(),
            })
        }
    }

    pub async fn send(&self, plan: Plan, body: Vec<u8>) -> Sent {
        let started = Instant::now();
        let mut request = self
            .client
            .request(plan.method, plan.url)
            .headers(plan.headers)
            .timeout(LOCAL_TIMEOUT);
        if !body.is_empty() {
            request = request.body(body);
        }
        let outcome = match request.send().await {
            Ok(response) => Outcome::Delivered {
                status: response.status().as_u16(),
            },
            Err(error) => Outcome::Failed {
                reason: describe(&error.without_url()),
            },
        };
        Sent {
            outcome,
            elapsed: started.elapsed(),
        }
    }
}

fn strip_prefix(path: &str, prefix: Option<&str>) -> String {
    let Some(prefix) = prefix.map(|prefix| prefix.trim_end_matches('/')) else {
        return path.to_owned();
    };
    let boundary = path
        .strip_prefix(prefix)
        .filter(|rest| rest.is_empty() || rest.starts_with('/'));
    match boundary {
        Some(rest) => rest.to_owned(),
        None => path.to_owned(),
    }
}

fn captured_headers(detail: &CaptureDetail) -> (HeaderMap, Vec<String>) {
    let listed: Vec<String> = detail
        .headers
        .iter()
        .filter(|entry| entry.name.eq_ignore_ascii_case(CONNECTION.as_str()))
        .flat_map(|entry| entry.value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect();
    let mut headers = HeaderMap::new();
    let mut dropped = Vec::new();
    for entry in &detail.headers {
        let Ok(name) = HeaderName::from_bytes(entry.name.as_bytes()) else {
            dropped.push(format!("{} (invalid name)", entry.name));
            continue;
        };
        if STRIPPED_HEADERS.contains(&name.as_str())
            || listed.iter().any(|token| token == name.as_str())
        {
            continue;
        }
        match HeaderValue::from_bytes(entry.value.as_bytes()) {
            Ok(value) => {
                headers.append(name, value);
            }
            Err(_) => dropped.push(format!("{name} (invalid value)")),
        }
    }
    (headers, dropped)
}

#[cfg(test)]
mod tests {
    use crate::model::HeaderEntry;

    use super::*;

    fn replayer(target: &str, strip: Option<&str>, rules: HeaderRules) -> Replayer {
        Replayer::new(ReplayConfig {
            target: Url::parse(target).expect("target"),
            strip_prefix: strip.map(str::to_owned),
            headers: rules,
        })
        .expect("client")
    }

    fn detail(path: &str, query: &str, headers: &[(&str, &str)]) -> CaptureDetail {
        CaptureDetail {
            method: "POST".to_owned(),
            path: path.to_owned(),
            query_string: query.to_owned(),
            host: "webhooks.live".to_owned(),
            headers: headers
                .iter()
                .map(|(name, value)| HeaderEntry {
                    name: (*name).to_owned(),
                    value: (*value).to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn appends_the_sub_path_and_the_raw_query_to_the_target() {
        let plan = replayer("http://localhost:3000/hooks/", None, HeaderRules::default())
            .plan(&detail("/404/orders", "a=1&b=%20x&a=2", &[]))
            .expect("plan");
        assert_eq!(
            plan.url.as_str(),
            "http://localhost:3000/hooks/404/orders?a=1&b=%20x&a=2"
        );
    }

    #[test]
    fn keeps_an_empty_sub_path_and_an_empty_query_off_the_url() {
        let plan = replayer("http://localhost:3000", None, HeaderRules::default())
            .plan(&detail("", "", &[]))
            .expect("plan");
        assert_eq!(plan.url.as_str(), "http://localhost:3000/");
    }

    #[test]
    fn strips_a_prefix_only_at_a_segment_boundary() {
        let replayer = replayer(
            "http://localhost:3000",
            Some("/stripe"),
            HeaderRules::default(),
        );
        let stripped = replayer
            .plan(&detail("/stripe/events", "", &[]))
            .expect("plan");
        let untouched = replayer
            .plan(&detail("/stripeish/events", "", &[]))
            .expect("plan");
        assert_eq!(stripped.url.path(), "/events");
        assert_eq!(untouched.url.path(), "/stripeish/events");
    }

    #[test]
    fn refuses_a_path_that_climbs_out_of_the_target_prefix() {
        let result = replayer("http://localhost:3000/hooks", None, HeaderRules::default())
            .plan(&detail("/../admin", "", &[]));
        assert!(matches!(result, Err(ReplayError::PathEscapesTarget { .. })));
        let encoded = replayer("http://localhost:3000/hooks", None, HeaderRules::default())
            .plan(&detail("/%2e%2e/admin", "", &[]));
        assert!(matches!(
            encoded,
            Err(ReplayError::PathEscapesTarget { .. })
        ));
    }

    #[test]
    fn drops_hop_by_hop_headers_host_and_content_length() {
        let plan = replayer("http://localhost:3000", None, HeaderRules::default())
            .plan(&detail(
                "/",
                "",
                &[
                    ("Host", "webhooks.live"),
                    ("Connection", "keep-alive, X-Private"),
                    ("X-Private", "1"),
                    ("Transfer-Encoding", "chunked"),
                    ("Content-Length", "5"),
                    ("Content-Type", "application/json"),
                    ("X-Signature", "abc"),
                ],
            ))
            .expect("plan");
        let names: Vec<&str> = plan.headers.keys().map(HeaderName::as_str).collect();
        assert_eq!(names, vec!["content-type", "x-signature"]);
    }

    #[test]
    fn keeps_duplicate_headers_in_order() {
        let plan = replayer("http://localhost:3000", None, HeaderRules::default())
            .plan(&detail("/", "", &[("X-Tag", "a"), ("X-Tag", "b")]))
            .expect("plan");
        let values: Vec<&str> = plan
            .headers
            .get_all("x-tag")
            .iter()
            .map(|value| value.to_str().expect("ascii"))
            .collect();
        assert_eq!(values, vec!["a", "b"]);
    }

    #[test]
    fn applies_keep_host_then_removals_then_overrides() {
        let rules = HeaderRules {
            keep_host: true,
            remove: vec![HeaderName::from_static("x-remove")],
            set: vec![(
                HeaderName::from_static("x-signature"),
                HeaderValue::from_static("local"),
            )],
        };
        let plan = replayer("http://localhost:3000", None, rules)
            .plan(&detail(
                "/",
                "",
                &[
                    ("X-Remove", "1"),
                    ("X-Signature", "abc"),
                    ("X-Signature", "def"),
                ],
            ))
            .expect("plan");
        assert_eq!(plan.headers.get(HOST).expect("host"), "webhooks.live");
        assert!(plan.headers.get("x-remove").is_none());
        let signatures: Vec<&str> = plan
            .headers
            .get_all("x-signature")
            .iter()
            .map(|value| value.to_str().expect("ascii"))
            .collect();
        assert_eq!(signatures, vec!["local"]);
    }

    #[test]
    fn reports_a_header_it_cannot_replay_instead_of_hiding_it() {
        let plan = replayer("http://localhost:3000", None, HeaderRules::default())
            .plan(&detail(
                "/",
                "",
                &[("Bad Name", "x"), ("X-Ok", "line\nbreak")],
            ))
            .expect("plan");
        assert!(plan.headers.is_empty());
        assert_eq!(plan.dropped.len(), 2);
    }

    #[test]
    fn refuses_an_invalid_method() {
        let mut broken = detail("/", "", &[]);
        broken.method = "BAD METHOD".to_owned();
        let result = replayer("http://localhost:3000", None, HeaderRules::default()).plan(&broken);
        assert!(matches!(result, Err(ReplayError::Method(_))));
    }
}
