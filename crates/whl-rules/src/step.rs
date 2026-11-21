use std::collections::BTreeSet;

use serde_json::{Map, Value};
use whl_core::content_type::ContentType;
use whl_core::header::{HeaderName, is_token};
use whl_script::MAX_SOURCE_BYTES as MAX_SCRIPT_BYTES;
use whl_secrets::SecretName;
use whl_template::{RenderTarget, SecretPolicy};

use crate::check::{Checker, is_step_name, is_variable_name, join};
use crate::error::RuleErrorCode;
use crate::kind::StepKind;
use crate::limits::{
    DEFAULT_HTTP_TIMEOUT_MS, MAX_ALLOWLIST_ENTRIES, MAX_DELAY_MS, MAX_EXTRACT_GROUP,
    MAX_HTTP_HEADERS, MAX_HTTP_TIMEOUT_MS, MAX_INCREMENT_BY, MAX_METHOD_BYTES,
    MAX_RESPOND_BODY_BYTES, MAX_RESPOND_HEADERS, MAX_S3_PREFIX_BYTES, MAX_S3_REGION_BYTES,
    MAX_TARGET_URL_BYTES,
};
use crate::model::{Action, BodySource, Header, S3Content, Step};
use crate::pattern::PatternUse;

const COMMON_FIELDS: [&str; 2] = ["kind", "continue_on_error"];

#[derive(Clone, Copy)]
struct Slot {
    target: RenderTarget,
    secrets: SecretPolicy,
}

impl Slot {
    const fn local(target: RenderTarget) -> Self {
        Self {
            target,
            secrets: SecretPolicy::Forbidden,
        }
    }

    const fn outbound(target: RenderTarget) -> Self {
        Self {
            target,
            secrets: SecretPolicy::Allowed,
        }
    }
}

pub(crate) fn validate_step(
    checker: &mut Checker,
    value: &Value,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Step> {
    let map = checker.object(value, path)?;
    let text = checker.required_text(map, path, "kind")?;
    let Some(kind) = StepKind::parse(text) else {
        checker.fail(
            join(path, "kind"),
            RuleErrorCode::InvalidFormat,
            "is not a known step kind",
        );
        return None;
    };
    let mut allowed: Vec<&str> = COMMON_FIELDS.to_vec();
    if !kind.name_is_field() {
        allowed.push("name");
    }
    allowed.extend_from_slice(kind.fields());
    checker.known(map, path, &allowed);
    let name = if kind.name_is_field() {
        None
    } else {
        optional_step_name(checker, map, path, steps)?
    };
    let continue_on_error = checker.boolean(map, path, "continue_on_error", false);
    let action = action(checker, kind, map, path, steps)?;
    Some(Step {
        name,
        continue_on_error,
        action,
    })
}

fn optional_step_name(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Option<String>> {
    let field = join(path, "name");
    match map.get("name").filter(|value| !value.is_null()) {
        None => Some(None),
        Some(Value::String(name)) => {
            if !is_step_name(name) {
                checker.fail(
                    field,
                    RuleErrorCode::InvalidFormat,
                    "must match ^[a-z][a-z0-9_]{0,31}$",
                );
                None
            } else if steps.contains(name) {
                checker.fail(
                    field,
                    RuleErrorCode::Duplicate,
                    "is already used by another step of this endpoint",
                );
                None
            } else {
                Some(Some(name.clone()))
            }
        }
        Some(_) => {
            checker.fail(field, RuleErrorCode::InvalidFormat, "must be a string");
            None
        }
    }
}

fn action(
    checker: &mut Checker,
    kind: StepKind,
    map: &Map<String, Value>,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Action> {
    Some(match kind {
        StepKind::Respond => respond(checker, map, path, steps)?,
        StepKind::SetHeader => {
            let name = header_name(checker, map, path, "name");
            let value = template_field(
                checker,
                map,
                path,
                "value",
                Slot::local(RenderTarget::HeaderValue),
                steps,
            );
            Action::SetHeader {
                name: name?,
                value: value?,
            }
        }
        StepKind::RemoveHeader => Action::RemoveHeader {
            name: header_name(checker, map, path, "name")?,
        },
        StepKind::Delay => Action::Delay {
            ms: u32::try_from(ranged(checker, map, path, "ms", 0, MAX_DELAY_MS, None)?).ok()?,
        },
        StepKind::Stop => Action::Stop,
        StepKind::Extract => extract(checker, map, path, steps)?,
        StepKind::SetVar => {
            let into = run_variable(checker, map, path, "into");
            let value = template_field(
                checker,
                map,
                path,
                "value",
                Slot::local(RenderTarget::Body),
                steps,
            );
            Action::SetVar {
                into: into?,
                value: value?,
            }
        }
        StepKind::Transform => {
            let into = into_target(checker, map, path);
            let template = template_field(
                checker,
                map,
                path,
                "template",
                Slot::local(RenderTarget::Body),
                steps,
            );
            Action::Transform {
                into: into?,
                template: template?,
            }
        }
        StepKind::SetStore => {
            let name = run_variable(checker, map, path, "name");
            let value = template_field(
                checker,
                map,
                path,
                "value",
                Slot::local(RenderTarget::Body),
                steps,
            );
            Action::SetStore {
                name: name?,
                value: value?,
            }
        }
        StepKind::Increment => {
            let counter = run_variable(checker, map, path, "counter");
            let by = ranged(
                checker,
                map,
                path,
                "by",
                -MAX_INCREMENT_BY,
                MAX_INCREMENT_BY,
                Some(1),
            );
            Action::Increment {
                counter: counter?,
                by: by?,
            }
        }
        StepKind::Script => script(checker, map, path)?,
        StepKind::Http => http(checker, map, path, steps)?,
        StepKind::Forward => forward(checker, map, path)?,
        StepKind::Notify => {
            let channel_id = checker.uuid(map, path, "channel_id").filter(|id| {
                let known = checker.knows_channel(id);
                if !known {
                    checker.fail(
                        join(path, "channel_id"),
                        RuleErrorCode::InvalidFormat,
                        "notification channel is not found on this endpoint",
                    );
                }
                known
            });
            let message = optional_template(
                checker,
                map,
                path,
                "message",
                Slot::local(RenderTarget::Body),
                steps,
            );
            Action::Notify {
                channel_id: channel_id?,
                message: message?,
            }
        }
        StepKind::StoreS3 => store_s3(checker, map, path)?,
    })
}

fn respond(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Action> {
    let status = template_field(
        checker,
        map,
        path,
        "status",
        Slot::local(RenderTarget::Status),
        steps,
    );
    let content_type = optional_template(
        checker,
        map,
        path,
        "content_type",
        Slot::local(RenderTarget::HeaderValue),
        steps,
    );
    let headers = header_list(
        checker,
        map,
        path,
        "headers",
        Slot::local(RenderTarget::HeaderValue),
        steps,
        MAX_RESPOND_HEADERS,
    );
    let body = optional_template(
        checker,
        map,
        path,
        "body",
        Slot::local(RenderTarget::Body),
        steps,
    );
    if let Some(Some(text)) = &body
        && text.len() > MAX_RESPOND_BODY_BYTES
    {
        checker.fail(
            join(path, "body"),
            RuleErrorCode::TooLong,
            format!("must be at most {MAX_RESPOND_BODY_BYTES} bytes"),
        );
    }
    Some(Action::Respond {
        status: status?,
        content_type: content_type?,
        headers: headers?,
        body: body?.unwrap_or_default(),
    })
}

fn template_field(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
    slot: Slot,
    steps: &BTreeSet<String>,
) -> Option<String> {
    let source = checker.required_text(map, path, key)?;
    checker
        .template(&join(path, key), source, slot.target, slot.secrets, steps)
        .then(|| source.to_owned())
}

fn optional_template(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
    slot: Slot,
    steps: &BTreeSet<String>,
) -> Option<Option<String>> {
    match checker.text(map, path, key) {
        None => Some(None),
        Some(source) => checker
            .template(&join(path, key), source, slot.target, slot.secrets, steps)
            .then(|| Some(source.to_owned())),
    }
}

fn header_name(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
) -> Option<String> {
    let text = checker.required_text(map, path, key)?;
    match HeaderName::parse(text) {
        Ok(name) => Some(name.unpack().to_owned()),
        Err(_) => {
            checker.fail(
                join(path, key),
                RuleErrorCode::InvalidFormat,
                "must be a valid header name",
            );
            None
        }
    }
}

fn header_list(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
    slot: Slot,
    steps: &BTreeSet<String>,
    max: usize,
) -> Option<Vec<Header>> {
    let Some(items) = checker.array(map, path, key) else {
        return Some(Vec::new());
    };
    if items.len() > max {
        checker.fail(
            join(path, key),
            RuleErrorCode::TooMany,
            format!("must have at most {max} entries"),
        );
        return None;
    }
    let mut headers = Vec::with_capacity(items.len());
    let mut valid = true;
    for (index, item) in items.iter().enumerate() {
        let item_path = format!("{}.{index}", join(path, key));
        let Some(item_map) = checker.object(item, &item_path) else {
            valid = false;
            continue;
        };
        checker.known(item_map, &item_path, &["name", "value"]);
        let name = header_name(checker, item_map, &item_path, "name");
        let value = template_field(checker, item_map, &item_path, "value", slot, steps);
        match (name, value) {
            (Some(name), Some(value)) => headers.push(Header { name, value }),
            _ => valid = false,
        }
    }
    valid.then_some(headers)
}

fn ranged(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
    min: i64,
    max: i64,
    default: Option<i64>,
) -> Option<i64> {
    let value = match map.get(key).filter(|value| !value.is_null()) {
        None => {
            if default.is_none() {
                checker.fail(join(path, key), RuleErrorCode::Required, "is required");
            }
            return default;
        }
        Some(value) => value,
    };
    let Some(number) = value.as_i64() else {
        checker.fail(
            join(path, key),
            RuleErrorCode::InvalidFormat,
            "must be an integer",
        );
        return None;
    };
    if !(min..=max).contains(&number) {
        checker.fail(
            join(path, key),
            RuleErrorCode::OutOfRange,
            format!("must be between {min} and {max}"),
        );
        return None;
    }
    Some(number)
}

fn run_variable(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
) -> Option<String> {
    let text = checker.required_text(map, path, key)?;
    if is_variable_name(text) {
        Some(text.to_owned())
    } else {
        checker.fail(
            join(path, key),
            RuleErrorCode::InvalidFormat,
            "must match ^[a-z_][a-z0-9_]{0,63}$",
        );
        None
    }
}

fn into_target(checker: &mut Checker, map: &Map<String, Value>, path: &str) -> Option<String> {
    let text = checker.required_text(map, path, "into")?;
    if text == "payload" || is_variable_name(text) {
        Some(text.to_owned())
    } else {
        checker.fail(
            join(path, "into"),
            RuleErrorCode::InvalidFormat,
            "must be a run variable name or payload",
        );
        None
    }
}

fn secret_name(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
) -> Option<String> {
    let text = checker.required_text(map, path, key)?;
    match SecretName::parse(text) {
        Ok(name) if checker.knows_secret(name.unpack()) => Some(name.unpack().to_owned()),
        Ok(_) => {
            checker.fail(
                join(path, key),
                RuleErrorCode::InvalidFormat,
                format!("secret {text} is not defined on this endpoint"),
            );
            None
        }
        Err(_) => {
            checker.fail(
                join(path, key),
                RuleErrorCode::InvalidFormat,
                "must be a secret name",
            );
            None
        }
    }
}

fn method(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
) -> Option<String> {
    let text = checker.required_text(map, path, key)?;
    if text.len() > MAX_METHOD_BYTES {
        checker.fail(
            join(path, key),
            RuleErrorCode::TooLong,
            format!("must be at most {MAX_METHOD_BYTES} bytes"),
        );
        None
    } else if text.is_empty() || !is_token(text) {
        checker.fail(
            join(path, key),
            RuleErrorCode::InvalidFormat,
            "must be a valid HTTP method",
        );
        None
    } else {
        Some(text.to_ascii_uppercase())
    }
}

fn optional_content_type(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
) -> Option<Option<String>> {
    let Some(text) = checker.text(map, path, key) else {
        return Some(None);
    };
    match ContentType::parse(text) {
        Ok(parsed) => Some(Some(parsed.unpack().to_owned())),
        Err(error) => {
            for code in error.codes() {
                checker.fail(
                    join(path, key),
                    RuleErrorCode::InvalidFormat,
                    code.to_string(),
                );
            }
            None
        }
    }
}

fn allowlist(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    key: &str,
) -> Option<Vec<String>> {
    let Some(items) = checker.array(map, path, key) else {
        return Some(Vec::new());
    };
    if items.len() > MAX_ALLOWLIST_ENTRIES {
        checker.fail(
            join(path, key),
            RuleErrorCode::TooMany,
            format!("must have at most {MAX_ALLOWLIST_ENTRIES} entries"),
        );
        return None;
    }
    let mut names = Vec::with_capacity(items.len());
    for item in items {
        match item.as_str().and_then(|text| HeaderName::parse(text).ok()) {
            Some(name) => names.push(name.unpack().to_owned()),
            None => {
                checker.fail(
                    join(path, key),
                    RuleErrorCode::InvalidFormat,
                    "must contain header names",
                );
                return None;
            }
        }
    }
    Some(names)
}

fn extract(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Action> {
    let from = checker
        .required_text(map, path, "from")
        .filter(|text| checker.variable_path(&join(path, "from"), text, steps))
        .map(str::to_owned);
    let json_path = checker.text(map, path, "json_path");
    let regex = checker.text(map, path, "regex");
    let into = run_variable(checker, map, path, "into");
    let selector = match (json_path, regex) {
        (Some(json), None) => {
            if map.get("group").is_some_and(|value| !value.is_null()) {
                checker.fail(
                    join(path, "group"),
                    RuleErrorCode::NotAllowed,
                    "is only allowed with regex",
                );
            }
            checker.json_path(&join(path, "json_path"), json);
            Some(Selector::JsonPath(json.to_owned()))
        }
        (None, Some(pattern)) => {
            checker.pattern(&join(path, "regex"), pattern, PatternUse::Extract);
            let group = ranged(checker, map, path, "group", 0, MAX_EXTRACT_GROUP, Some(1))
                .and_then(|group| u32::try_from(group).ok());
            Some(Selector::Regex(pattern.to_owned(), group?))
        }
        (None, None) => {
            checker.fail(path, RuleErrorCode::Required, "must set json_path or regex");
            None
        }
        (Some(_), Some(_)) => {
            checker.fail(
                path,
                RuleErrorCode::InvalidFormat,
                "must set only one of json_path or regex",
            );
            None
        }
    };
    let (json_path, regex, group) = match selector? {
        Selector::JsonPath(json) => (Some(json), None, None),
        Selector::Regex(pattern, group) => (None, Some(pattern), Some(group)),
    };
    Some(Action::Extract {
        from: from?,
        json_path,
        regex,
        group,
        into: into?,
    })
}

enum Selector {
    JsonPath(String),
    Regex(String, u32),
}

fn script(checker: &mut Checker, map: &Map<String, Value>, path: &str) -> Option<Action> {
    let source = checker.required_text(map, path, "source")?;
    let field = join(path, "source");
    if source.len() > MAX_SCRIPT_BYTES {
        checker.fail(
            field,
            RuleErrorCode::TooLong,
            format!("must be at most {MAX_SCRIPT_BYTES} bytes"),
        );
        return None;
    }
    checker.script(&field, source);
    Some(Action::Script {
        source: source.to_owned(),
    })
}

fn http(
    checker: &mut Checker,
    map: &Map<String, Value>,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Action> {
    let method = method(checker, map, path, "method");
    let url = template_field(
        checker,
        map,
        path,
        "url",
        Slot::outbound(RenderTarget::Body),
        steps,
    );
    if url.as_ref().is_some_and(String::is_empty) {
        checker.fail(
            join(path, "url"),
            RuleErrorCode::Required,
            "must not be empty",
        );
    }
    let headers = header_list(
        checker,
        map,
        path,
        "headers",
        Slot::outbound(RenderTarget::HeaderValue),
        steps,
        MAX_HTTP_HEADERS,
    );
    let body = optional_template(
        checker,
        map,
        path,
        "body",
        Slot::outbound(RenderTarget::Body),
        steps,
    );
    let timeout = ranged(
        checker,
        map,
        path,
        "timeout_ms",
        1,
        MAX_HTTP_TIMEOUT_MS,
        Some(DEFAULT_HTTP_TIMEOUT_MS),
    );
    let follow_redirects = checker.boolean(map, path, "follow_redirects", true);
    Some(Action::Http {
        method: method?,
        url: url?,
        headers: headers?,
        body: body?,
        timeout_ms: u32::try_from(timeout?).ok()?,
        follow_redirects,
    })
}

fn forward(checker: &mut Checker, map: &Map<String, Value>, path: &str) -> Option<Action> {
    let rule_id = checker.optional_uuid(map, path, "forward_rule_id");
    let target = checker.text(map, path, "target_url");
    let body_from = body_source(checker, map, path)?;
    match (rule_id, target) {
        (Some(id), None) => {
            if !checker.knows_forward_rule(&id) {
                checker.fail(
                    join(path, "forward_rule_id"),
                    RuleErrorCode::InvalidFormat,
                    "forward rule is not found on this endpoint",
                );
                return None;
            }
            for key in ["method", "content_type", "header_allowlist"] {
                if map.get(key).is_some_and(|value| !is_empty_value(value)) {
                    checker.fail(
                        join(path, key),
                        RuleErrorCode::NotAllowed,
                        "must be absent when forward_rule_id is set",
                    );
                }
            }
            Some(Action::Forward {
                forward_rule_id: Some(id),
                target_url: None,
                method: None,
                content_type: None,
                header_allowlist: Vec::new(),
                body_from,
            })
        }
        (None, Some(url)) => {
            if url.is_empty() || url.len() > MAX_TARGET_URL_BYTES {
                checker.fail(
                    join(path, "target_url"),
                    RuleErrorCode::InvalidFormat,
                    format!("must be between 1 and {MAX_TARGET_URL_BYTES} bytes"),
                );
                return None;
            }
            let method = if map.get("method").is_some_and(|value| !value.is_null()) {
                Some(method(checker, map, path, "method")?)
            } else {
                None
            };
            let content_type = optional_content_type(checker, map, path, "content_type")?;
            let header_allowlist = allowlist(checker, map, path, "header_allowlist")?;
            Some(Action::Forward {
                forward_rule_id: None,
                target_url: Some(url.to_owned()),
                method,
                content_type,
                header_allowlist,
                body_from,
            })
        }
        _ => {
            checker.fail(
                path,
                RuleErrorCode::InvalidFormat,
                "must set exactly one of forward_rule_id or target_url",
            );
            None
        }
    }
}

fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn body_source(checker: &mut Checker, map: &Map<String, Value>, path: &str) -> Option<BodySource> {
    match checker.text(map, path, "body_from") {
        None | Some("capture") => Some(BodySource::Capture),
        Some("payload") => Some(BodySource::Payload),
        Some(_) => {
            checker.fail(
                join(path, "body_from"),
                RuleErrorCode::InvalidFormat,
                "must be capture or payload",
            );
            None
        }
    }
}

fn store_s3(checker: &mut Checker, map: &Map<String, Value>, path: &str) -> Option<Action> {
    let bucket_url = checker.required_text(map, path, "bucket_url");
    if let Some(url) = bucket_url
        && (url.is_empty() || url.len() > MAX_TARGET_URL_BYTES)
    {
        checker.fail(
            join(path, "bucket_url"),
            RuleErrorCode::InvalidFormat,
            format!("must be between 1 and {MAX_TARGET_URL_BYTES} bytes"),
        );
    }
    let prefix = checker.text(map, path, "prefix").unwrap_or("");
    if prefix.len() > MAX_S3_PREFIX_BYTES || prefix.chars().any(char::is_control) {
        checker.fail(
            join(path, "prefix"),
            RuleErrorCode::InvalidFormat,
            format!("must be at most {MAX_S3_PREFIX_BYTES} bytes without control characters"),
        );
    }
    let region = checker.required_text(map, path, "region");
    if let Some(region) = region {
        let valid = !region.is_empty()
            && region.len() <= MAX_S3_REGION_BYTES
            && region
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
        if !valid {
            checker.fail(
                join(path, "region"),
                RuleErrorCode::InvalidFormat,
                "must be a region name",
            );
        }
    }
    let access_key = secret_name(checker, map, path, "access_key_secret");
    let secret_key = secret_name(checker, map, path, "secret_key_secret");
    let content = match checker.required_text(map, path, "content") {
        Some("capture_json") => Some(S3Content::CaptureJson),
        Some("body") => Some(S3Content::Body),
        Some(_) => {
            checker.fail(
                join(path, "content"),
                RuleErrorCode::InvalidFormat,
                "must be capture_json or body",
            );
            None
        }
        None => None,
    };
    Some(Action::StoreS3 {
        bucket_url: bucket_url?.to_owned(),
        prefix: prefix.to_owned(),
        region: region?.to_owned(),
        access_key_secret: access_key?,
        secret_key_secret: secret_key?,
        content: content?,
    })
}
