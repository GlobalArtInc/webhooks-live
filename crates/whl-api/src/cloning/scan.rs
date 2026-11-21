use std::collections::BTreeSet;

use serde_json::Value;
use whl_secrets::SecretName;
use whl_template::{Policy, RenderTarget, Template};

const NAME_KEYS: &[&str] = &[
    "secret_name",
    "token_secret",
    "access_key_secret",
    "secret_key_secret",
];

pub fn referenced_secrets(values: &[&Value]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for value in values {
        visit(value, &mut names);
    }
    names
}

fn visit(value: &Value, names: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => scan_text(text, names),
        Value::Array(items) => {
            for item in items {
                visit(item, names);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                if NAME_KEYS.contains(&key.as_str())
                    && let Some(name) = item.as_str().and_then(|text| SecretName::parse(text).ok())
                {
                    names.insert(name.unpack().to_owned());
                }
                visit(item, names);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn scan_text(text: &str, names: &mut BTreeSet<String>) {
    if !text.contains("{{") {
        return;
    }
    if let Ok(template) = Template::parse(text, &Policy::outbound(RenderTarget::Body)) {
        names.extend(template.referenced_secrets());
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn names_are_found_in_templates_and_in_name_fields() {
        let value = json!({
            "url": "https://hooks.example/{{ secret.API_TOKEN }}/x",
            "alt": "{{ secret[\"BRACKET_KEY\"] }}",
            "headers": [{"name": "x", "value": "Bearer {{secret.OTHER_1}}"}],
            "secret_name": "SIGNING",
            "text": "secret.lowercase is not a reference"
        });
        let names = referenced_secrets(&[&value]);
        let expected: BTreeSet<String> = ["API_TOKEN", "BRACKET_KEY", "OTHER_1", "SIGNING"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(names, expected);
    }
}
