use serde_json::{Map, Value};

use super::export::Selection;
use super::prepare::Overrides;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::input::parse_endpoint_input;

const REQUEST_KEYS: &[&str] = &["title", "include"];

pub struct CloneRequest {
    pub overrides: Overrides,
    pub selection: Selection,
}

pub fn parse_clone_request(bytes: &[u8]) -> Result<CloneRequest, ApiError> {
    if bytes.is_empty() {
        return Ok(CloneRequest {
            overrides: Overrides::none(),
            selection: Selection::ALL,
        });
    }
    let object = match serde_json::from_slice::<Value>(bytes)
        .map_err(|_| ApiError::InvalidJson("the request body is not valid JSON"))?
    {
        Value::Object(object) => object,
        _ => {
            return Err(ApiError::InvalidJson(
                "the request body must be a JSON object",
            ));
        }
    };
    let mut violations = Vec::new();
    for key in object
        .keys()
        .filter(|key| !REQUEST_KEYS.contains(&key.as_str()))
    {
        violations.push(FieldViolation::new(
            key.as_str(),
            FieldCode::NotAllowed,
            "is not a known field",
        ));
    }
    let selection = match object.get("include") {
        None => Selection::ALL,
        Some(value) => selection_of(value, &mut violations),
    };
    let overrides = match object.get("title") {
        None => Overrides::none(),
        Some(title) => {
            let mut only = Map::new();
            only.insert("title".to_owned(), title.clone());
            let encoded = serde_json::to_vec(&Value::Object(only))
                .map_err(|_| ApiError::InternalFault("the clone title could not be encoded"))?;
            Overrides::from_input(parse_endpoint_input(&encoded, false)?)
        }
    };
    violations.extend(overrides.violations.iter().cloned());
    if violations.is_empty() {
        Ok(CloneRequest {
            overrides: Overrides {
                patch: overrides.patch,
                violations: Vec::new(),
            },
            selection,
        })
    } else {
        Err(ApiError::Validation(violations))
    }
}

fn selection_of(value: &Value, violations: &mut Vec<FieldViolation>) -> Selection {
    let mut selection = Selection {
        endpoint: false,
        forward_rules: false,
        rules: false,
        channels: false,
        schedules: false,
    };
    let Some(items) = value.as_array() else {
        violations.push(FieldViolation::new(
            "include",
            FieldCode::InvalidFormat,
            "must be an array of part names",
        ));
        return selection;
    };
    for (index, item) in items.iter().enumerate() {
        let field = format!("include.{index}");
        match item.as_str() {
            Some("response") => selection.endpoint = true,
            Some("forward_rules") => selection.forward_rules = true,
            Some("rules") => selection.rules = true,
            Some("channels") => selection.channels = true,
            Some("schedules") => selection.schedules = true,
            Some("secrets") => violations.push(FieldViolation::new(
                field,
                FieldCode::NotAllowed,
                "secret values are never copied; set them again on the clone",
            )),
            _ => violations.push(FieldViolation::new(
                field,
                FieldCode::InvalidFormat,
                "must be response, forward_rules, rules, channels or schedules",
            )),
        }
    }
    selection
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_body_selects_every_part() {
        let request = parse_clone_request(b"").expect("an empty body is a valid request");
        assert_eq!(request.selection, Selection::ALL);
    }

    #[test]
    fn include_names_the_parts_and_refuses_secrets() {
        let request = parse_clone_request(br#"{"include": ["rules", "channels"]}"#)
            .expect("known parts are accepted");
        assert!(request.selection.rules && request.selection.channels);
        assert!(!request.selection.endpoint && !request.selection.schedules);
        let refused = parse_clone_request(br#"{"include": ["secrets"]}"#);
        assert!(matches!(refused, Err(ApiError::Validation(_))));
    }
}
