use serde_json::Value;
use whl_notify::{
    KeyError, MAX_ENCODED_KEY_CHARS, PushHosts, PushUrlRefusal, decode_auth_secret, decode_p256dh,
};

use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::notify::object_of;

const TOP_LEVEL_KEYS: [&str; 3] = ["endpoint", "keys", "expirationTime"];
const KEY_FIELDS: [&str; 2] = ["p256dh", "auth"];

pub struct ParsedSubscription {
    pub endpoint: String,
    pub host: String,
    pub p256dh: Vec<u8>,
    pub auth: Vec<u8>,
}

pub fn parse_subscription(bytes: &[u8], hosts: &PushHosts) -> Result<ParsedSubscription, ApiError> {
    let object = object_of(bytes)?;
    let mut violations = Vec::new();
    for key in object
        .keys()
        .filter(|key| !TOP_LEVEL_KEYS.contains(&key.as_str()))
    {
        violations.push(FieldViolation::new(
            key.as_str(),
            FieldCode::NotAllowed,
            "is not a known field",
        ));
    }
    if let Some(value) = object.get("expirationTime")
        && !value.is_null()
        && !value.is_number()
    {
        violations.push(FieldViolation::new(
            "expirationTime",
            FieldCode::InvalidFormat,
            "must be a number or null",
        ));
    }
    let endpoint = endpoint_of(object.get("endpoint"), hosts, &mut violations);
    let keys = keys_of(object.get("keys"), &mut violations);
    match (endpoint, keys) {
        (Some(endpoint), Some(keys)) if violations.is_empty() => Ok(ParsedSubscription {
            endpoint: endpoint.url,
            host: endpoint.host,
            p256dh: keys.p256dh,
            auth: keys.auth,
        }),
        _ => Err(ApiError::Validation(violations)),
    }
}

struct ParsedEndpoint {
    url: String,
    host: String,
}

struct ParsedKeys {
    p256dh: Vec<u8>,
    auth: Vec<u8>,
}

fn endpoint_of(
    value: Option<&Value>,
    hosts: &PushHosts,
    violations: &mut Vec<FieldViolation>,
) -> Option<ParsedEndpoint> {
    let text = match value {
        None | Some(Value::Null) => {
            violations.push(FieldViolation::new(
                "endpoint",
                FieldCode::Required,
                "is required",
            ));
            return None;
        }
        Some(Value::String(text)) => text,
        Some(_) => {
            violations.push(FieldViolation::new(
                "endpoint",
                FieldCode::InvalidFormat,
                "must be a string",
            ));
            return None;
        }
    };
    let refusal = match hosts.check_url(text) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) => {
                return Some(ParsedEndpoint {
                    url: parsed.as_str().to_owned(),
                    host: host.to_owned(),
                });
            }
            None => PushUrlRefusal::Shape,
        },
        Err(refusal) => refusal,
    };
    let code = match refusal {
        PushUrlRefusal::TooLong => FieldCode::TooLong,
        PushUrlRefusal::Invalid | PushUrlRefusal::Shape => FieldCode::InvalidFormat,
        PushUrlRefusal::Host(_) => FieldCode::HostNotAllowed,
    };
    violations.push(FieldViolation::new("endpoint", code, refusal.to_string()));
    None
}

fn keys_of(value: Option<&Value>, violations: &mut Vec<FieldViolation>) -> Option<ParsedKeys> {
    let object = match value {
        None | Some(Value::Null) => {
            violations.push(FieldViolation::new(
                "keys",
                FieldCode::Required,
                "is required",
            ));
            return None;
        }
        Some(Value::Object(object)) => object,
        Some(_) => {
            violations.push(FieldViolation::new(
                "keys",
                FieldCode::InvalidFormat,
                "must be an object",
            ));
            return None;
        }
    };
    for key in object
        .keys()
        .filter(|key| !KEY_FIELDS.contains(&key.as_str()))
    {
        violations.push(FieldViolation::new(
            format!("keys.{key}"),
            FieldCode::NotAllowed,
            "is not a known field",
        ));
    }
    let p256dh = key_field(
        object.get("p256dh"),
        "keys.p256dh",
        decode_p256dh,
        violations,
    );
    let auth = key_field(
        object.get("auth"),
        "keys.auth",
        decode_auth_secret,
        violations,
    );
    Some(ParsedKeys {
        p256dh: p256dh?,
        auth: auth?,
    })
}

fn key_field(
    value: Option<&Value>,
    field: &str,
    decode: fn(&str) -> Result<Vec<u8>, KeyError>,
    violations: &mut Vec<FieldViolation>,
) -> Option<Vec<u8>> {
    match value {
        None | Some(Value::Null) => {
            violations.push(FieldViolation::new(
                field,
                FieldCode::Required,
                "is required",
            ));
            None
        }
        Some(Value::String(text)) => match decode(text) {
            Ok(bytes) => Some(bytes),
            Err(error) => {
                let code = if text.len() > MAX_ENCODED_KEY_CHARS {
                    FieldCode::TooLong
                } else {
                    FieldCode::InvalidFormat
                };
                violations.push(FieldViolation::new(field, code, error.to_string()));
                None
            }
        },
        Some(_) => {
            violations.push(FieldViolation::new(
                field,
                FieldCode::InvalidFormat,
                "must be a string",
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_POINT: &str =
        "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
    const VALID_SEED: &str = "BTBZMqHH6r4Tts7J_aSIgg";

    fn body(endpoint: &str) -> Vec<u8> {
        serde_json::json!({
            "endpoint": endpoint,
            "expirationTime": null,
            "keys": { "p256dh": VALID_POINT, "auth": VALID_SEED },
        })
        .to_string()
        .into_bytes()
    }

    fn fields(error: ApiError) -> Vec<(String, FieldCode)> {
        match error {
            ApiError::Validation(violations) => violations
                .iter()
                .map(|violation| (violation.field().to_owned(), violation.code()))
                .collect(),
            other => panic!("expected validation errors, got {other}"),
        }
    }

    #[test]
    fn a_browser_subscription_json_is_accepted() {
        let parsed = parse_subscription(
            &body("https://fcm.googleapis.com/fcm/send/abc"),
            &PushHosts::vendors(),
        )
        .expect("a valid subscription");
        assert_eq!(parsed.endpoint, "https://fcm.googleapis.com/fcm/send/abc");
        assert_eq!(parsed.host, "fcm.googleapis.com");
        assert_eq!(parsed.p256dh.len(), 65);
        assert_eq!(parsed.auth.len(), 16);
    }

    #[test]
    fn a_foreign_host_and_bad_keys_are_each_named_in_their_field() {
        let bad = serde_json::json!({
            "endpoint": "https://evil.test/push",
            "keys": { "p256dh": "AAAA", "auth": "AAAA", "extra": "x" },
            "other": 1,
        })
        .to_string()
        .into_bytes();
        let error = parse_subscription(&bad, &PushHosts::vendors())
            .err()
            .expect("the subscription is refused");
        let mut found = fields(error);
        found.sort_by(|left, right| left.0.cmp(&right.0));
        assert_eq!(
            found,
            [
                ("endpoint".to_owned(), FieldCode::HostNotAllowed),
                ("keys.auth".to_owned(), FieldCode::InvalidFormat),
                ("keys.extra".to_owned(), FieldCode::NotAllowed),
                ("keys.p256dh".to_owned(), FieldCode::InvalidFormat),
                ("other".to_owned(), FieldCode::NotAllowed),
            ]
        );
    }
}
