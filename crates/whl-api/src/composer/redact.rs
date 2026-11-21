use serde_json::Value;
use url::{Url, form_urlencoded};

pub const REDACTED: &str = "[redacted]";
pub const TOO_DEEP: &str = "[too deep]";
pub const ELIDED_PATH: &str = "/\u{2026}";
pub const MAX_JSON_DEPTH: usize = 32;
pub const TOKEN_SEGMENT_MIN: usize = 16;

const SENSITIVE_PARTS: [&str; 14] = [
    "authorization",
    "cookie",
    "token",
    "secret",
    "passw",
    "passphrase",
    "passcode",
    "apikey",
    "credential",
    "signature",
    "bearer",
    "privatekey",
    "session",
    "jwt",
];

const SENSITIVE_WORDS: [&str; 7] = ["key", "auth", "pwd", "sig", "sid", "otp", "pass"];
const REFERENCE_HEADERS: [&str; 4] = ["location", "content-location", "referer", "origin"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPolicy {
    Origin,
    Tokens { first_segment: bool },
}

#[derive(Debug, PartialEq, Eq)]
pub struct RedactedBody {
    pub bytes: Vec<u8>,
    pub capped: bool,
}

fn words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for character in name.chars() {
        if !character.is_ascii_alphanumeric() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if character.is_ascii_uppercase() && previous_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        previous_lower = character.is_ascii_lowercase();
        current.push(character.to_ascii_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

pub fn is_sensitive_name(name: &str) -> bool {
    let squashed: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect();
    if SENSITIVE_PARTS.iter().any(|part| squashed.contains(part)) {
        return true;
    }
    words(name)
        .iter()
        .any(|word| SENSITIVE_WORDS.contains(&word.as_str()))
}

pub fn redact_header_value(name: &str, value: &str, policy: PathPolicy) -> String {
    if is_sensitive_name(name) {
        return REDACTED.to_owned();
    }
    if REFERENCE_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
        return redact_reference(value, policy);
    }
    redact_embedded_url(value, policy)
}

fn redact_reference(value: &str, policy: PathPolicy) -> String {
    let stripped = strip_userinfo(value);
    if Url::parse(&stripped).is_ok() {
        return redact_url(&stripped, policy);
    }
    if policy == PathPolicy::Origin {
        return ELIDED_PATH.to_owned();
    }
    let without_fragment = stripped.split('#').next().unwrap_or_default();
    let (path, query) = match without_fragment.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (without_fragment, None),
    };
    let path = redact_path(path, policy);
    match query {
        Some(query) => format!("{path}?{}", redact_form(query.as_bytes())),
        None => path,
    }
}

fn redact_embedded_url(value: &str, policy: PathPolicy) -> String {
    if value.contains("://") {
        redact_url(&strip_userinfo(value), policy)
    } else {
        value.to_owned()
    }
}

pub fn strip_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find("//") {
        let (head, tail) = rest.split_at(position + 2);
        out.push_str(head);
        let end = tail
            .find(|character: char| character == '/' || character.is_whitespace())
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(at) => rest = &tail[at + 1..],
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

fn is_token_segment(segment: &str) -> bool {
    segment.len() >= TOKEN_SEGMENT_MIN
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':'))
}

fn redact_path(path: &str, policy: PathPolicy) -> String {
    let first_segment = matches!(
        policy,
        PathPolicy::Tokens {
            first_segment: true
        }
    );
    let mut out = String::with_capacity(path.len());
    for (index, segment) in path.split('/').enumerate() {
        if index > 0 {
            out.push('/');
        }
        let hide =
            (first_segment && index == 1 && !segment.is_empty()) || is_token_segment(segment);
        if hide {
            out.push_str(REDACTED);
        } else {
            out.push_str(segment);
        }
    }
    out
}

pub fn redact_url(raw: &str, policy: PathPolicy) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return match policy {
            PathPolicy::Origin => ELIDED_PATH.to_owned(),
            PathPolicy::Tokens { .. } => {
                let kept = raw.split(['?', '#']).next().unwrap_or_default();
                strip_userinfo(kept)
            }
        };
    };
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    if policy == PathPolicy::Origin {
        url.set_query(None);
        url.set_path("/");
        let origin = url.as_str().trim_end_matches('/').to_owned();
        return format!("{origin}{ELIDED_PATH}");
    }
    let path = redact_path(url.path(), policy);
    let query = url.query().map(|query| redact_form(query.as_bytes()));
    let mut out = format!("{}://", url.scheme());
    if let Some(host) = url.host_str() {
        out.push_str(host);
    }
    if let Some(port) = url.port() {
        out.push_str(&format!(":{port}"));
    }
    out.push_str(&path);
    if let Some(query) = query {
        out.push('?');
        out.push_str(&query);
    }
    out
}

fn redact_form(bytes: &[u8]) -> String {
    let mut out = form_urlencoded::Serializer::new(String::new());
    for (name, value) in form_urlencoded::parse(bytes) {
        if is_sensitive_name(&name) {
            out.append_pair(&name, REDACTED);
        } else {
            out.append_pair(&name, &value);
        }
    }
    out.finish()
}

fn redact_json(value: &mut Value, depth: usize, policy: PathPolicy) {
    if depth > MAX_JSON_DEPTH {
        *value = Value::String(TOO_DEEP.to_owned());
        return;
    }
    match value {
        Value::Object(map) => {
            let pair_is_sensitive = map
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(is_sensitive_name)
                && map.contains_key("value");
            for (name, inner) in map.iter_mut() {
                if is_sensitive_name(name) || (pair_is_sensitive && name == "value") {
                    *inner = Value::String(REDACTED.to_owned());
                } else {
                    redact_json(inner, depth + 1, policy);
                }
            }
        }
        Value::Array(items) => {
            for inner in items {
                redact_json(inner, depth + 1, policy);
            }
        }
        Value::String(text) => {
            *text = redact_embedded_url(text, policy);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn media_essence(content_type: Option<&str>) -> String {
    content_type
        .and_then(|text| text.split(';').next())
        .map(|essence| essence.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

pub fn redact_body(
    content_type: Option<&str>,
    bytes: &[u8],
    cap: usize,
    policy: PathPolicy,
) -> Option<RedactedBody> {
    let essence = media_essence(content_type);
    let mut redacted = if essence == "application/json" || essence.ends_with("+json") {
        let mut value: Value = serde_json::from_slice(bytes).ok()?;
        redact_json(&mut value, 0, policy);
        serde_json::to_vec(&value).ok()?
    } else if essence == "application/x-www-form-urlencoded" {
        redact_form(bytes).into_bytes()
    } else {
        return None;
    };
    let capped = redacted.len() > cap;
    if capped {
        let mut end = cap;
        while end > 0 && !is_char_start(&redacted, end) {
            end -= 1;
        }
        redacted.truncate(end);
    }
    Some(RedactedBody {
        bytes: redacted,
        capped,
    })
}

fn is_char_start(bytes: &[u8], index: usize) -> bool {
    bytes
        .get(index)
        .is_none_or(|byte| (byte & 0b1100_0000) != 0b1000_0000)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKENS: PathPolicy = PathPolicy::Tokens {
        first_segment: false,
    };

    #[test]
    fn sensitive_names_cover_credentials_and_leave_plain_names() {
        for name in [
            "Authorization",
            "cookie",
            "X-API-Key",
            "x-auth-token",
            "access_token",
            "clientSecret",
            "userPassword",
            "myApiKey",
            "passphrase",
            "passcode",
            "password",
            "api_key",
            "sig",
        ] {
            assert!(is_sensitive_name(name), "{name}");
        }
        for name in ["content-type", "accept", "page", "monkey", "user", "q"] {
            assert!(!is_sensitive_name(name), "{name}");
        }
    }

    #[test]
    fn urls_lose_sensitive_query_values_userinfo_and_fragments() {
        let redacted = redact_url(
            "https://user:pw@example.com/p?a=1&token=abc&Api-Key=z#frag",
            TOKENS,
        );
        assert_eq!(
            redacted,
            "https://example.com/p?a=1&token=%5Bredacted%5D&Api-Key=%5Bredacted%5D"
        );
    }

    #[test]
    fn token_like_path_segments_are_hidden_for_user_history() {
        let secret = "x".repeat(24);
        let hook = format!("https://hooks.example.test/services/TEST/FIXTURE/{secret}");
        assert_eq!(
            redact_url(&hook, TOKENS),
            "https://hooks.example.test/services/TEST/FIXTURE/[redacted]"
        );
        let bot = format!(
            "https://bots.example.test/bot{}:{}/send",
            "1".repeat(9),
            "a".repeat(30)
        );
        assert_eq!(
            redact_url(&bot, TOKENS),
            "https://bots.example.test/[redacted]/send"
        );
        let own = "https://whl.test/7k2m9q4xw8c1d5f3h6j0n2p4/hook";
        assert_eq!(
            redact_url(
                own,
                PathPolicy::Tokens {
                    first_segment: true
                }
            ),
            "https://whl.test/[redacted]/hook"
        );
        assert_eq!(
            redact_url("https://example.com/orders/42", TOKENS),
            "https://example.com/orders/42"
        );
    }

    #[test]
    fn endpoint_history_keeps_only_the_origin() {
        assert_eq!(
            redact_url(
                "https://chat.example.test:8443/api/hooks/1/tok?wait=true",
                PathPolicy::Origin
            ),
            "https://chat.example.test:8443/\u{2026}"
        );
        assert_eq!(
            redact_header_value("Location", "/next?x=1", PathPolicy::Origin),
            ELIDED_PATH
        );
    }

    #[test]
    fn userinfo_is_stripped_even_from_values_that_are_not_urls() {
        assert_eq!(
            redact_header_value("x-dsn", "dsn=postgres://u:pw@h/db", TOKENS),
            "dsn=postgres://h/db"
        );
    }

    #[test]
    fn json_bodies_lose_sensitive_values_at_any_depth_and_other_types_are_dropped() {
        let body = br#"{"a":{"password":"x","list":[{"token":"t","ok":1}]},"b":"https://h/?secret=1","h":[{"name":"Authorization","value":"v"}]}"#;
        let redacted = redact_body(Some("application/json; charset=utf-8"), body, 1024, TOKENS)
            .expect("json is kept");
        let text = String::from_utf8(redacted.bytes).expect("utf8");
        assert!(!text.contains("\"x\"") && !text.contains("\"t\""), "{text}");
        assert!(!text.contains("\"v\""), "{text}");
        assert!(text.contains("\"ok\":1"), "{text}");
        assert!(!text.contains("secret=1"), "{text}");
        assert!(!redacted.capped);
        assert_eq!(
            redact_body(Some("text/plain"), b"token=abc", 1024, TOKENS),
            None
        );
        assert_eq!(
            redact_body(Some("application/json"), b"{broken", 1024, TOKENS),
            None
        );
    }

    #[test]
    fn pretty_printed_json_is_not_flagged_as_capped_but_a_real_cut_is() {
        let pretty = b"{\n  \"a\": 1,\n  \"b\": 2\n}";
        let kept = redact_body(Some("application/json"), pretty, 64, TOKENS).expect("kept");
        assert!(!kept.capped);
        let cut = redact_body(Some("application/json"), pretty, 4, TOKENS).expect("kept");
        assert!(cut.capped);
        assert_eq!(cut.bytes.len(), 4);
    }
}
