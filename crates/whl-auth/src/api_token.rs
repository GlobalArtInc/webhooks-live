use std::fmt;
use std::time::Duration;

use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::{IdGenerationError, random_bytes};
use whl_store::{ApiTokenLookup, Store, TokenScope, Uuid};
use whl_telemetry::series;

use crate::current_user::CurrentUser;
use crate::error::AuthRejection;

pub const TOKEN_PREFIX: &str = "whl_";
pub const LOOKUP_PREFIX_CHARS: usize = 12;
pub const LAST_USED_INTERVAL: Duration = Duration::from_secs(60);

const RANDOM_BYTES: usize = 32;
const RANDOM_CHARS: usize = 52;
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
const BEARER_SCHEME: &[u8] = b"bearer";
const DUMMY_HASH: [u8; 32] = [0_u8; 32];

fn base32(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 8 / 5 + 1);
    let mut buffer = 0_u16;
    let mut held = 0_u32;
    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        held += 8;
        while held >= 5 {
            held -= 5;
            text.push(char::from(ALPHABET[usize::from((buffer >> held) & 0x1f)]));
        }
        buffer &= (1 << held) - 1;
    }
    if held > 0 {
        text.push(char::from(
            ALPHABET[usize::from((buffer << (5 - held)) & 0x1f)],
        ));
    }
    text
}

#[derive(Clone)]
pub struct ApiTokenSecret(String);

impl ApiTokenSecret {
    pub fn generate() -> Result<Self, IdGenerationError> {
        let bytes = random_bytes::<RANDOM_BYTES>()?;
        Ok(Self(format!("{TOKEN_PREFIX}{}", base32(&bytes))))
    }

    pub fn parse(text: &str) -> Option<Self> {
        let random = text.strip_prefix(TOKEN_PREFIX)?;
        let valid = random.len() == RANDOM_CHARS && random.bytes().all(|b| ALPHABET.contains(&b));
        valid.then(|| Self(text.to_owned()))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }

    pub fn lookup_prefix(&self) -> &str {
        self.0.get(..LOOKUP_PREFIX_CHARS).unwrap_or(self.0.as_str())
    }

    pub fn digest(&self) -> ApiTokenHash {
        ApiTokenHash(Sha256::digest(self.0.as_bytes()).into())
    }
}

impl fmt::Debug for ApiTokenSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiTokenSecret(<redacted>)")
    }
}

#[derive(Clone)]
pub struct ApiTokenHash([u8; 32]);

impl ApiTokenHash {
    pub fn unpack(&self) -> &[u8] {
        &self.0
    }

    fn matches(&self, stored: &[u8]) -> bool {
        bool::from(self.0.as_slice().ct_eq(stored))
    }
}

impl fmt::Debug for ApiTokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiTokenHash(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenGrant {
    id: Uuid,
    scopes: Vec<TokenScope>,
    endpoint_ids: Option<Vec<EndpointId>>,
}

impl TokenGrant {
    pub fn new(id: Uuid, scopes: Vec<TokenScope>, endpoint_ids: Option<Vec<EndpointId>>) -> Self {
        Self {
            id,
            scopes,
            endpoint_ids,
        }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn scopes(&self) -> &[TokenScope] {
        &self.scopes
    }

    pub fn endpoint_limit(&self) -> Option<&[EndpointId]> {
        self.endpoint_ids.as_deref()
    }

    pub fn allows_scope(&self, scope: TokenScope) -> bool {
        self.scopes.contains(&scope)
    }

    pub fn allows_endpoint(&self, endpoint: &EndpointId) -> bool {
        self.endpoint_ids
            .as_ref()
            .is_none_or(|limit| limit.contains(endpoint))
    }
}

#[derive(Debug, Clone)]
pub(crate) enum BearerHeader {
    Absent,
    Present(Option<ApiTokenSecret>),
}

impl BearerHeader {
    pub(crate) fn is_present(&self) -> bool {
        matches!(self, Self::Present(_))
    }
}

fn strip_bearer_scheme(value: &[u8]) -> Option<&[u8]> {
    let (scheme, rest) = value.split_at_checked(BEARER_SCHEME.len())?;
    if !scheme.eq_ignore_ascii_case(BEARER_SCHEME) {
        return None;
    }
    match rest.first() {
        None | Some(b' ') => Some(rest),
        Some(_) => None,
    }
}

pub(crate) fn bearer(headers: &HeaderMap) -> BearerHeader {
    let values: Vec<&[u8]> = headers
        .get_all(header::AUTHORIZATION)
        .iter()
        .map(|value| value.as_bytes())
        .collect();
    let mut found = values.iter().filter_map(|value| strip_bearer_scheme(value));
    let Some(first) = found.next() else {
        return BearerHeader::Absent;
    };
    if values.len() > 1 {
        return BearerHeader::Present(None);
    }
    let secret = std::str::from_utf8(first.trim_ascii_start())
        .ok()
        .and_then(ApiTokenSecret::parse);
    BearerHeader::Present(secret)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Accepted,
    Unknown,
    Expired,
    Revoked,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Self::Accepted => "ok",
            Self::Unknown => "invalid",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }
}

fn record(outcome: Outcome) {
    metrics::counter!(series::TOKEN_AUTH_TOTAL, "result" => outcome.label()).increment(1);
}

fn select<'a>(digest: &ApiTokenHash, rows: &'a [ApiTokenLookup]) -> Option<&'a ApiTokenLookup> {
    if rows.is_empty() {
        let _ = digest.matches(&DUMMY_HASH);
        return None;
    }
    let mut found = None;
    for row in rows {
        if digest.matches(&row.token_hash) {
            found = Some(row);
        }
    }
    found
}

pub(crate) async fn resolve(
    store: &Store,
    secret: Option<&ApiTokenSecret>,
) -> Result<(CurrentUser, TokenGrant), AuthRejection> {
    let Some(secret) = secret else {
        record(Outcome::Unknown);
        return Err(AuthRejection::InvalidToken);
    };
    let digest = secret.digest();
    let rows = store
        .lookup_api_tokens(secret.lookup_prefix(), LAST_USED_INTERVAL)
        .await?;
    let found = select(&digest, &rows);
    let outcome = match found {
        None => Outcome::Unknown,
        Some(row) if row.revoked => Outcome::Revoked,
        Some(row) if row.expired => Outcome::Expired,
        Some(_) => Outcome::Accepted,
    };
    record(outcome);
    let Some(row) = found.filter(|_| outcome == Outcome::Accepted) else {
        return Err(AuthRejection::InvalidToken);
    };
    if row.touch_due {
        match store.touch_api_token(row.id, LAST_USED_INTERVAL).await {
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(token_id = %row.id, %error, "recording the API token use failed");
            }
        }
    }
    Ok((
        CurrentUser::new(row.user.clone()),
        TokenGrant::new(row.id, row.scopes.clone(), row.endpoint_ids.clone()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(values: &[&[u8]]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for value in values {
            map.append(
                header::AUTHORIZATION,
                HeaderValue::from_bytes(value).expect("a header value"),
            );
        }
        map
    }

    fn sample() -> ApiTokenSecret {
        ApiTokenSecret::generate().expect("entropy is available")
    }

    #[test]
    fn a_generated_token_has_the_documented_shape_and_parses_back() {
        let token = sample();
        assert!(token.unpack().starts_with(TOKEN_PREFIX));
        assert_eq!(token.unpack().len(), TOKEN_PREFIX.len() + RANDOM_CHARS);
        assert_eq!(token.lookup_prefix().len(), LOOKUP_PREFIX_CHARS);
        assert_eq!(
            ApiTokenSecret::parse(token.unpack()).map(|parsed| parsed.digest().unpack().to_vec()),
            Some(token.digest().unpack().to_vec())
        );
        assert_ne!(token.unpack(), sample().unpack());
    }

    #[test]
    fn the_base32_encoding_matches_a_known_vector() {
        assert_eq!(base32(&[0xff, 0x00, 0xff, 0x00, 0xff]), "74ap6ah7");
        assert_eq!(base32(&[0xff; 32]).len(), RANDOM_CHARS);
    }

    #[test]
    fn parse_rejects_wrong_prefix_length_and_alphabet() {
        let random = "a".repeat(RANDOM_CHARS);
        assert!(ApiTokenSecret::parse(&format!("whl_{random}")).is_some());
        for text in [
            String::new(),
            "whl_".to_owned(),
            format!("xhl_{random}"),
            format!("whl_{}", &random[1..]),
            format!("whl_{random}a"),
            format!("whl_{}1", &random[1..]),
            format!("whl_{}A", &random[1..]),
            format!("WHL_{random}"),
        ] {
            assert!(ApiTokenSecret::parse(&text).is_none(), "{text}");
        }
    }

    #[test]
    fn debug_output_never_carries_the_secret_part() {
        let token = sample();
        let rendered = format!("{token:?} {:?}", token.digest());
        assert!(!rendered.contains(&token.unpack()[LOOKUP_PREFIX_CHARS..]));
    }

    #[test]
    fn the_hash_comparison_accepts_only_the_exact_digest() {
        let token = sample();
        let digest = token.digest();
        assert!(digest.matches(digest.unpack()));
        assert!(!digest.matches(sample().digest().unpack()));
        assert!(!digest.matches(&digest.unpack()[..31]));
        assert!(!digest.matches(&[]));
    }

    #[test]
    fn the_bearer_header_is_found_case_insensitively_and_strictly() {
        let token = sample();
        let line = format!("Bearer {}", token.unpack());
        let lower = format!("bEaReR   {}", token.unpack());
        for value in [&line, &lower] {
            match bearer(&headers(&[value.as_bytes()])) {
                BearerHeader::Present(Some(parsed)) => {
                    assert_eq!(parsed.unpack(), token.unpack());
                }
                other => panic!("{other:?}"),
            }
        }
        assert!(matches!(bearer(&headers(&[])), BearerHeader::Absent));
        assert!(matches!(
            bearer(&headers(&[b"Basic dXNlcjpwYXNz"])),
            BearerHeader::Absent
        ));
        assert!(matches!(
            bearer(&headers(&[b"Bearerx abc"])),
            BearerHeader::Absent
        ));
        for value in [
            &b"Bearer"[..],
            b"Bearer ",
            b"Bearer junk",
            b"Bearer \xff\xfe",
        ] {
            assert!(matches!(
                bearer(&headers(&[value])),
                BearerHeader::Present(None)
            ));
        }
    }

    #[test]
    fn two_authorization_headers_with_a_bearer_are_refused() {
        let line = format!("Bearer {}", sample().unpack());
        assert!(matches!(
            bearer(&headers(&[line.as_bytes(), b"Basic dXNlcjpwYXNz"])),
            BearerHeader::Present(None)
        ));
        assert!(matches!(
            bearer(&headers(&[line.as_bytes(), line.as_bytes()])),
            BearerHeader::Present(None)
        ));
    }

    #[test]
    fn a_grant_checks_scopes_and_the_endpoint_limit() {
        let first = EndpointId::parse("0123456789abcdefghjkmnpq").expect("an id");
        let second = EndpointId::parse("0123456789abcdefghjkmnpr").expect("an id");
        let open = TokenGrant::new(Uuid::nil(), vec![TokenScope::Read], None);
        assert!(open.allows_scope(TokenScope::Read));
        assert!(!open.allows_scope(TokenScope::Send));
        assert!(open.allows_endpoint(&first));
        let limited = TokenGrant::new(
            Uuid::nil(),
            vec![TokenScope::Read],
            Some(vec![first.clone()]),
        );
        assert!(limited.allows_endpoint(&first));
        assert!(!limited.allows_endpoint(&second));
    }
}
