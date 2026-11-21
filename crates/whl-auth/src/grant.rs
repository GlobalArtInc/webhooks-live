use std::fmt;
use std::time::Duration;

use axum::http::HeaderMap;
use whl_core::entropy::IdGenerationError;

use crate::token::{RandomToken, TokenHash};

pub const GRANT_HEADER: &str = "x-webhooks-live-grant";
pub const GRANT_QUERY_KEY: &str = "grant";
pub const GRANT_LIFETIME: Duration = Duration::from_secs(12 * 3600);

#[derive(Clone, PartialEq, Eq)]
pub struct ViewGrant(RandomToken);

impl ViewGrant {
    pub fn generate() -> Result<Self, IdGenerationError> {
        RandomToken::generate().map(Self)
    }

    pub fn parse(text: &str) -> Option<Self> {
        RandomToken::parse(text).map(Self)
    }

    pub fn unpack(&self) -> &str {
        self.0.unpack()
    }

    pub fn digest(&self) -> TokenHash {
        self.0.digest()
    }
}

impl fmt::Debug for ViewGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ViewGrant(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRule {
    NotRequired,
    Header,
    HeaderOrQuery,
}

pub fn presented(headers: &HeaderMap, query: Option<&str>) -> Option<ViewGrant> {
    if let Some(value) = headers.get(GRANT_HEADER) {
        return value.to_str().ok().and_then(ViewGrant::parse);
    }
    let query = query?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(GRANT_QUERY_KEY)?.strip_prefix('='))
        .and_then(ViewGrant::parse)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn grant() -> ViewGrant {
        ViewGrant::generate().expect("entropy is available")
    }

    #[test]
    fn generated_grants_are_distinct_and_parse_back() {
        let first = grant();
        assert_ne!(first, grant());
        assert_eq!(ViewGrant::parse(first.unpack()), Some(first.clone()));
        assert_eq!(ViewGrant::parse("short"), None);
        assert!(!format!("{first:?}").contains(first.unpack()));
    }

    #[test]
    fn the_header_wins_and_a_bad_header_never_falls_back_to_the_query() {
        let header_grant = grant();
        let query_grant = grant();
        let query = format!("grant={}", query_grant.unpack());
        let mut headers = HeaderMap::new();
        assert_eq!(
            presented(&headers, Some(&query)),
            Some(query_grant.clone()),
            "the query is read when there is no header"
        );
        headers.insert(
            GRANT_HEADER,
            HeaderValue::from_str(header_grant.unpack()).expect("a header value"),
        );
        assert_eq!(presented(&headers, Some(&query)), Some(header_grant));
        headers.insert(GRANT_HEADER, HeaderValue::from_static("not-a-grant"));
        assert_eq!(presented(&headers, Some(&query)), None);
    }

    #[test]
    fn the_query_is_matched_by_exact_key_and_the_first_pair_only() {
        let first = grant();
        let second = grant();
        let headers = HeaderMap::new();
        let crowded = format!(
            "last_event_id=4&xgrant={}&grant={}&grant={}",
            second.unpack(),
            first.unpack(),
            second.unpack()
        );
        assert_eq!(presented(&headers, Some(&crowded)), Some(first));
        assert_eq!(presented(&headers, Some("grant=")), None);
        assert_eq!(presented(&headers, Some("grant")), None);
        assert_eq!(presented(&headers, None), None);
    }
}
