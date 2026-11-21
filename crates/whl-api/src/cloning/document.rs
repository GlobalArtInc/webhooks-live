use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use whl_secrets::{MAX_SECRETS_PER_ENDPOINT, SecretName};
use whl_store::forward::MAX_RULES_PER_ENDPOINT as MAX_FORWARD_RULES;
use whl_store::notify::MAX_CHANNELS_PER_ENDPOINT;

use crate::error::{ApiError, FieldCode, FieldViolation};

pub const DOCUMENT_VERSION: i64 = 1;
pub const MAX_DOCUMENT_BYTES: usize = 512 * 1024;
pub const MAX_DOCUMENT_SCHEDULES: usize = 20;

const DOCUMENT_KEYS: &[&str] = &[
    "version",
    "endpoint",
    "forward_rules",
    "rules",
    "channels",
    "schedules",
    "secret_names",
];

#[derive(Clone, Default)]
pub struct ConfigDocument {
    pub endpoint: Map<String, Value>,
    pub forward_rules: Vec<Value>,
    pub rules: Vec<Value>,
    pub channels: Vec<Value>,
    pub schedules: Vec<Value>,
    pub secret_names: BTreeSet<String>,
}

impl ConfigDocument {
    pub fn to_value(&self) -> Value {
        json!({
            "version": DOCUMENT_VERSION,
            "endpoint": self.endpoint,
            "forward_rules": self.forward_rules,
            "rules": self.rules,
            "channels": self.channels,
            "schedules": self.schedules,
            "secret_names": self.secret_names,
        })
    }

    pub fn parse(value: &Value) -> Result<Self, ApiError> {
        let Some(object) = value.as_object() else {
            return Err(violation(
                "config",
                FieldCode::InvalidFormat,
                "must be an object",
            ));
        };
        let size = serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_DOCUMENT_BYTES {
            return Err(violation(
                "config",
                FieldCode::TooLong,
                format!("must be at most {MAX_DOCUMENT_BYTES} bytes"),
            ));
        }
        let mut found = Vec::new();
        for key in object
            .keys()
            .filter(|key| !DOCUMENT_KEYS.contains(&key.as_str()))
        {
            found.push(FieldViolation::new(
                format!("config.{key}"),
                FieldCode::NotAllowed,
                "is not a known field",
            ));
        }
        if object.get("version").and_then(Value::as_i64) != Some(DOCUMENT_VERSION) {
            found.push(FieldViolation::new(
                "config.version",
                FieldCode::InvalidFormat,
                format!("must be {DOCUMENT_VERSION}"),
            ));
        }
        let endpoint = match object.get("endpoint") {
            Some(Value::Object(endpoint)) => endpoint.clone(),
            Some(_) => {
                found.push(FieldViolation::new(
                    "config.endpoint",
                    FieldCode::InvalidFormat,
                    "must be an object",
                ));
                Map::new()
            }
            None => {
                found.push(FieldViolation::new(
                    "config.endpoint",
                    FieldCode::Required,
                    "is required",
                ));
                Map::new()
            }
        };
        let forward_rules = objects(object, "forward_rules", MAX_FORWARD_RULES, &mut found);
        let rules = objects(
            object,
            "rules",
            whl_rules::MAX_RULES_PER_ENDPOINT,
            &mut found,
        );
        let channels = objects(object, "channels", MAX_CHANNELS_PER_ENDPOINT, &mut found);
        let schedules = objects(object, "schedules", MAX_DOCUMENT_SCHEDULES, &mut found);
        let secret_names = secret_names(object, &mut found);
        if found.is_empty() {
            Ok(Self {
                endpoint,
                forward_rules,
                rules,
                channels,
                schedules,
                secret_names,
            })
        } else {
            Err(ApiError::Validation(found))
        }
    }
}

fn violation(field: &str, code: FieldCode, message: impl Into<String>) -> ApiError {
    ApiError::Validation(vec![FieldViolation::new(field, code, message)])
}

fn objects(
    object: &Map<String, Value>,
    key: &str,
    max: usize,
    found: &mut Vec<FieldViolation>,
) -> Vec<Value> {
    let field = format!("config.{key}");
    match object.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) if items.len() > max => {
            found.push(FieldViolation::new(
                field,
                FieldCode::TooMany,
                format!("must have at most {max} entries"),
            ));
            Vec::new()
        }
        Some(Value::Array(items)) => {
            let mut kept = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                if item.is_object() {
                    kept.push(item.clone());
                } else {
                    found.push(FieldViolation::new(
                        format!("{field}.{index}"),
                        FieldCode::InvalidFormat,
                        "must be an object",
                    ));
                }
            }
            kept
        }
        Some(_) => {
            found.push(FieldViolation::new(
                field,
                FieldCode::InvalidFormat,
                "must be an array",
            ));
            Vec::new()
        }
    }
}

fn secret_names(object: &Map<String, Value>, found: &mut Vec<FieldViolation>) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let items = match object.get("secret_names") {
        None | Some(Value::Null) => return names,
        Some(Value::Array(items)) => items,
        Some(_) => {
            found.push(FieldViolation::new(
                "config.secret_names",
                FieldCode::InvalidFormat,
                "must be an array of secret names",
            ));
            return names;
        }
    };
    if items.len() > MAX_SECRETS_PER_ENDPOINT {
        found.push(FieldViolation::new(
            "config.secret_names",
            FieldCode::TooMany,
            format!("must have at most {MAX_SECRETS_PER_ENDPOINT} entries"),
        ));
        return names;
    }
    for (index, item) in items.iter().enumerate() {
        match item.as_str().map(SecretName::parse) {
            Some(Ok(name)) => {
                names.insert(name.unpack().to_owned());
            }
            _ => found.push(FieldViolation::new(
                format!("config.secret_names.{index}"),
                FieldCode::InvalidFormat,
                "must be a secret name",
            )),
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn more_secret_names_than_an_endpoint_can_hold_are_refused() {
        let names: Vec<String> = (0..=MAX_SECRETS_PER_ENDPOINT)
            .map(|index| format!("NAME_{index}"))
            .collect();
        let value = json!({"version": 1, "endpoint": {}, "secret_names": names});

        assert!(matches!(
            ConfigDocument::parse(&value),
            Err(ApiError::Validation(_))
        ));
    }
}
