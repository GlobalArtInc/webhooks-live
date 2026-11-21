use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::json;
use whl_secrets::{
    MAX_JSON_DEPTH, MAX_REDACT_INPUT_BYTES, MAX_REDACT_SECRETS, OVERSIZE_PLACEHOLDER, RedactError,
    Redactor, Secret, SecretName,
};

const TOKEN: &str = "tok_live_9f8e7d6c";
const OTHER: &str = "hunter2-hunter2";

fn pair(name: &str, value: &str) -> (SecretName, Secret) {
    (
        SecretName::parse(name).expect("a valid name"),
        Secret::new(value.to_owned()).expect("a valid value"),
    )
}

fn redactor(pairs: &[(SecretName, Secret)]) -> Redactor {
    Redactor::new(pairs.iter().map(|(name, value)| (name, value))).expect("builds")
}

fn single() -> Redactor {
    redactor(&[pair("API_TOKEN", TOKEN)])
}

#[test]
fn a_value_inside_a_string_is_replaced_by_its_name() {
    let redactor = single();

    let text = format!("POST failed: Authorization: Bearer {TOKEN} was refused");
    let result = redactor.redact_str(&text);

    assert_eq!(
        result,
        "POST failed: Authorization: Bearer [secret:API_TOKEN] was refused"
    );
}

#[test]
fn every_occurrence_and_every_secret_is_replaced() {
    let redactor = redactor(&[pair("API_TOKEN", TOKEN), pair("PASSWORD", OTHER)]);

    let text = format!("{TOKEN} {OTHER} {TOKEN}");
    let result = redactor.redact_str(&text);

    assert_eq!(
        result,
        "[secret:API_TOKEN] [secret:PASSWORD] [secret:API_TOKEN]"
    );
}

#[test]
fn base64_and_percent_encoded_forms_are_replaced_too() {
    let redactor = redactor(&[pair("KEY", "p@ss w/rd+1=")]);
    let standard = STANDARD.encode("p@ss w/rd+1=");
    let url_safe = URL_SAFE_NO_PAD.encode("p@ss w/rd+1=");

    assert_eq!(
        redactor.redact_str(&format!("b={standard}")),
        "b=[secret:KEY]"
    );
    assert_eq!(
        redactor.redact_str(&format!("b={url_safe}")),
        "b=[secret:KEY]"
    );
    assert_eq!(
        redactor.redact_str("q=p%40ss%20w%2Frd%2B1%3D"),
        "q=[secret:KEY]"
    );
    assert_eq!(
        redactor.redact_str("q=p%40ss%20w%2frd%2b1%3d"),
        "q=[secret:KEY]"
    );
    assert_eq!(
        redactor.redact_str("q=p%40ss+w%2Frd%2B1%3D"),
        "q=[secret:KEY]"
    );
}

#[test]
fn values_shorter_than_four_bytes_are_not_redacted() {
    let redactor = redactor(&[pair("PIN", "abc")]);

    assert!(redactor.is_empty());
    assert_eq!(redactor.redact_str("abc abc"), "abc abc");
}

#[test]
fn text_without_a_secret_is_returned_unchanged_and_borrowed() {
    let redactor = single();
    let text = "nothing to hide here";

    let result = redactor.redact_str(text);

    assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    assert_eq!(result, text);
}

#[test]
fn overlapping_secrets_leave_no_fragment_behind() {
    let redactor = redactor(&[pair("LONG", "abcdefgh"), pair("SHORT", "defghijk")]);

    let result = redactor.redact_str("xx abcdefghijk yy");

    assert!(!result.contains("abc"));
    assert!(!result.contains("ijk"));
    assert!(result.starts_with("xx [secret:"));
    assert!(result.ends_with("] yy"));
}

#[test]
fn json_strings_keys_and_numbers_are_redacted_at_any_depth() {
    let redactor = redactor(&[pair("API_TOKEN", TOKEN), pair("PIN", "12345678")]);
    let document = json!({
        "headers": [{"name": "Authorization", "value": format!("Bearer {TOKEN}")}],
        TOKEN: "key",
        "n": 12_345_678,
        "keep": [1, true, null, "plain"],
    });

    let result = redactor.redact_json(&document);

    assert_eq!(
        result,
        json!({
            "headers": [{"name": "Authorization", "value": "Bearer [secret:API_TOKEN]"}],
            "[secret:API_TOKEN]": "key",
            "n": "[secret:PIN]",
            "keep": [1, true, null, "plain"],
        })
    );
}

#[test]
fn redaction_cost_is_bounded_for_large_and_deep_input() {
    let redactor = single();
    let large = format!("{TOKEN}{}", "a".repeat(MAX_REDACT_INPUT_BYTES));

    assert_eq!(redactor.redact_str(&large), OVERSIZE_PLACEHOLDER);

    let mut deep = json!(TOKEN);
    for _ in 0..(MAX_JSON_DEPTH + 8) {
        deep = json!([deep]);
    }
    let rendered = redactor.redact_json(&deep).to_string();
    assert!(!rendered.contains(TOKEN));
    assert!(rendered.contains(OVERSIZE_PLACEHOLDER));

    let wide = json!(vec![TOKEN.repeat(1000); 1000]);
    let rendered = redactor.redact_json(&wide).to_string();
    assert!(!rendered.contains(TOKEN));
}

#[test]
fn more_than_the_documented_number_of_secrets_is_refused() {
    let pairs: Vec<_> = (0..=MAX_REDACT_SECRETS)
        .map(|index| pair(&format!("S{index}"), &format!("value-{index:04}")))
        .collect();

    let error =
        Redactor::new(pairs.iter().map(|(name, value)| (name, value))).expect_err("one too many");

    assert_eq!(error, RedactError::TooManySecrets);
}

#[test]
fn an_empty_redactor_passes_everything_through() {
    let redactor = redactor(&[]);

    assert_eq!(redactor.redact_str(TOKEN), TOKEN);
    assert_eq!(
        redactor.redact_json(&json!({"a": TOKEN})),
        json!({"a": TOKEN})
    );
}
