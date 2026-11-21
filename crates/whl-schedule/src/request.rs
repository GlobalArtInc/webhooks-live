use std::collections::BTreeSet;

use http::{HeaderName, Method};
use whl_store::schedules::ScheduleRequest;
use whl_template::{ErrorCode, Policy, RenderTarget, SecretPolicy, StepPolicy, Template};

pub const MAX_HEADERS: usize = 20;
pub const MAX_HEADER_NAME_BYTES: usize = 64;
pub const MAX_HEADER_VALUE_BYTES: usize = 1024;
pub const MAX_BODY_BYTES: usize = 64 * 1024;
pub const MAX_URL_BYTES: usize = whl_forward::MAX_URL_BYTES;
pub const METHODS: [&str; 7] = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];
pub const RESERVED_HEADER_PREFIX: &str = "x-webhooks-live-";

pub const SECRET_FILTERS: [&str; 4] = ["sha256", "hmac_sha256", "base64", "url"];

const RESERVED_HEADERS: [&str; 11] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authorization",
    "te",
    "trailer",
    "upgrade",
    "expect",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    Required,
    InvalidFormat,
    TooLong,
    TooMany,
    NotAllowed,
    TemplateInvalid,
    SecretNotAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub field: String,
    pub kind: ViolationKind,
    pub message: String,
}

impl Violation {
    fn new(field: impl Into<String>, kind: ViolationKind, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            kind,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompiledHeader {
    pub name: HeaderName,
    pub value: Template,
}

#[derive(Debug, Clone)]
pub struct CompiledRequest {
    pub method: Method,
    pub url: Template,
    pub static_url: Option<String>,
    pub headers: Vec<CompiledHeader>,
    pub body: Option<Template>,
}

impl CompiledRequest {
    pub fn compile(request: &ScheduleRequest) -> Result<Self, Vec<Violation>> {
        let mut violations = Vec::new();
        let method = check_method(&request.method, &mut violations);
        let url = check_url(&request.url, &mut violations);
        let headers = check_headers(request, &mut violations);
        let body = check_body(request.body.as_deref(), &mut violations);
        match (method, url) {
            (Some(method), Some(url)) if violations.is_empty() => Ok(Self {
                method,
                url,
                static_url: static_source(&request.url),
                headers,
                body,
            }),
            _ => Err(violations),
        }
    }

    pub fn secret_names(&self) -> BTreeSet<String> {
        let mut names = self.url.referenced_secrets();
        for header in &self.headers {
            names.extend(header.value.referenced_secrets());
        }
        names
    }
}

pub fn request_uses_secrets(request: &ScheduleRequest) -> bool {
    CompiledRequest::compile(request).map_or(true, |compiled| !compiled.secret_names().is_empty())
}

fn static_source(source: &str) -> Option<String> {
    (!source.contains("{{")).then(|| source.to_owned())
}

fn policy(target: RenderTarget, secrets: SecretPolicy) -> Policy {
    let mut policy = Policy::new(target, secrets);
    policy.steps = StepPolicy::Only(BTreeSet::new());
    policy
}

fn parse_template(
    source: &str,
    policy: &Policy,
    field: &str,
    violations: &mut Vec<Violation>,
) -> Option<Template> {
    match Template::parse(source, policy) {
        Ok(template) => Some(template),
        Err(error) => {
            let first = error.diagnostics.first();
            let kind = match first.map(|diagnostic| diagnostic.code) {
                Some(ErrorCode::SecretNotAllowed) => ViolationKind::SecretNotAllowed,
                _ => ViolationKind::TemplateInvalid,
            };
            let message = first.map_or_else(
                || "the template does not parse".to_owned(),
                |diagnostic| {
                    format!(
                        "{} at line {}, column {}",
                        diagnostic.message, diagnostic.position.line, diagnostic.position.column
                    )
                },
            );
            violations.push(Violation::new(field, kind, message));
            None
        }
    }
}

fn check_method(method: &str, violations: &mut Vec<Violation>) -> Option<Method> {
    if METHODS.contains(&method) {
        return Method::from_bytes(method.as_bytes()).ok();
    }
    violations.push(Violation::new(
        "request.method",
        ViolationKind::InvalidFormat,
        format!("must be one of {}", METHODS.join(", ")),
    ));
    None
}

fn check_url(url: &str, violations: &mut Vec<Violation>) -> Option<Template> {
    if url.is_empty() {
        violations.push(Violation::new(
            "request.url",
            ViolationKind::Required,
            "must not be empty",
        ));
        return None;
    }
    if url.len() > MAX_URL_BYTES {
        violations.push(Violation::new(
            "request.url",
            ViolationKind::TooLong,
            format!("must be at most {MAX_URL_BYTES} bytes"),
        ));
        return None;
    }
    let template = parse_template(
        url,
        &policy(RenderTarget::Body, SecretPolicy::Allowed),
        "request.url",
        violations,
    )?;
    check_secret_filters(&template, "request.url", violations).then_some(template)
}

fn check_secret_filters(template: &Template, field: &str, violations: &mut Vec<Violation>) -> bool {
    let refused: Vec<&str> = template
        .filters_over_secrets()
        .into_iter()
        .filter(|name| !SECRET_FILTERS.contains(name))
        .collect();
    if refused.is_empty() {
        return true;
    }
    violations.push(Violation::new(
        field,
        ViolationKind::SecretNotAllowed,
        format!(
            "the filters {} cannot be applied to a secret, only {} can",
            refused.join(", "),
            SECRET_FILTERS.join(", ")
        ),
    ));
    false
}

fn check_headers(
    request: &ScheduleRequest,
    violations: &mut Vec<Violation>,
) -> Vec<CompiledHeader> {
    if request.headers.len() > MAX_HEADERS {
        violations.push(Violation::new(
            "request.headers",
            ViolationKind::TooMany,
            format!("must have at most {MAX_HEADERS} entries"),
        ));
        return Vec::new();
    }
    let mut compiled = Vec::with_capacity(request.headers.len());
    let mut seen = BTreeSet::new();
    for (index, header) in request.headers.iter().enumerate() {
        let name_field = format!("request.headers.{index}.name");
        let value_field = format!("request.headers.{index}.value");
        let name = check_header_name(&header.name, &name_field, &mut seen, violations);
        let value = check_header_value(&header.value, &value_field, violations);
        if let (Some(name), Some(value)) = (name, value) {
            compiled.push(CompiledHeader { name, value });
        }
    }
    compiled
}

fn check_header_name(
    name: &str,
    field: &str,
    seen: &mut BTreeSet<String>,
    violations: &mut Vec<Violation>,
) -> Option<HeaderName> {
    if name.is_empty() || name.len() > MAX_HEADER_NAME_BYTES {
        violations.push(Violation::new(
            field,
            ViolationKind::InvalidFormat,
            format!("must be 1 to {MAX_HEADER_NAME_BYTES} bytes"),
        ));
        return None;
    }
    let Ok(parsed) = HeaderName::from_bytes(name.as_bytes()) else {
        violations.push(Violation::new(
            field,
            ViolationKind::InvalidFormat,
            "must be an HTTP header name",
        ));
        return None;
    };
    let lowered = parsed.as_str();
    if RESERVED_HEADERS.contains(&lowered) || lowered.starts_with(RESERVED_HEADER_PREFIX) {
        violations.push(Violation::new(
            field,
            ViolationKind::NotAllowed,
            "is managed by the service",
        ));
        return None;
    }
    if !seen.insert(lowered.to_owned()) {
        violations.push(Violation::new(
            field,
            ViolationKind::NotAllowed,
            "is listed twice",
        ));
        return None;
    }
    Some(parsed)
}

fn check_header_value(
    value: &str,
    field: &str,
    violations: &mut Vec<Violation>,
) -> Option<Template> {
    if value.len() > MAX_HEADER_VALUE_BYTES {
        violations.push(Violation::new(
            field,
            ViolationKind::TooLong,
            format!("must be at most {MAX_HEADER_VALUE_BYTES} bytes"),
        ));
        return None;
    }
    let template = parse_template(
        value,
        &policy(RenderTarget::HeaderValue, SecretPolicy::Allowed),
        field,
        violations,
    )?;
    check_secret_filters(&template, field, violations).then_some(template)
}

fn check_body(body: Option<&str>, violations: &mut Vec<Violation>) -> Option<Template> {
    let body = body?;
    if body.len() > MAX_BODY_BYTES {
        violations.push(Violation::new(
            "request.body",
            ViolationKind::TooLong,
            format!("must be at most {MAX_BODY_BYTES} bytes"),
        ));
        return None;
    }
    parse_template(
        body,
        &policy(RenderTarget::Body, SecretPolicy::Forbidden),
        "request.body",
        violations,
    )
}

#[cfg(test)]
mod tests {
    use whl_store::schedules::ScheduleHeader;

    use super::*;

    fn request() -> ScheduleRequest {
        ScheduleRequest {
            method: "GET".to_owned(),
            url: "https://example.com/health".to_owned(),
            headers: Vec::new(),
            body: None,
        }
    }

    fn fields(request: &ScheduleRequest) -> Vec<(String, ViolationKind)> {
        CompiledRequest::compile(request)
            .expect_err("the request is invalid")
            .into_iter()
            .map(|violation| (violation.field, violation.kind))
            .collect()
    }

    #[test]
    fn a_plain_request_compiles_and_a_dynamic_url_has_no_static_form() {
        let compiled = CompiledRequest::compile(&request()).expect("valid");
        assert_eq!(
            compiled.static_url.as_deref(),
            Some("https://example.com/health")
        );
        let mut dynamic = request();
        dynamic.url = "https://example.com/{{ now | date(\"%Y\") }}".to_owned();
        let compiled = CompiledRequest::compile(&dynamic).expect("valid");
        assert_eq!(compiled.static_url, None);
    }

    #[test]
    fn secrets_are_allowed_in_the_url_and_header_values_and_listed() {
        let mut with_secrets = request();
        with_secrets.url = "https://example.com/?k={{ secret.API_KEY }}".to_owned();
        with_secrets.headers.push(ScheduleHeader {
            name: "Authorization".to_owned(),
            value: "Bearer {{ secret.TOKEN }}".to_owned(),
        });
        let compiled = CompiledRequest::compile(&with_secrets).expect("valid");
        let names: Vec<String> = compiled.secret_names().into_iter().collect();
        assert_eq!(names, ["API_KEY", "TOKEN"]);
    }

    #[test]
    fn only_the_encoding_filters_may_touch_a_secret() {
        let mut request = request();
        request.url =
            "https://example.com/?a={{ secret.K | upper }}&b={{ secret.K | hex }}".to_owned();
        request.headers.push(ScheduleHeader {
            name: "X-Key".to_owned(),
            value: "{{ secret.K | replace(\"a\", \"b\") }}".to_owned(),
        });
        assert_eq!(
            fields(&request),
            [
                ("request.url".to_owned(), ViolationKind::SecretNotAllowed),
                (
                    "request.headers.0.value".to_owned(),
                    ViolationKind::SecretNotAllowed
                )
            ]
        );
        request.url = "https://example.com/?a={{ secret.K | url }}&b={{ secret.K | base64 }}&c={{ secret.K | sha256 }}".to_owned();
        request.headers.clear();
        assert!(CompiledRequest::compile(&request).is_ok());
    }

    #[test]
    fn a_secret_in_the_body_is_refused() {
        let mut bad = request();
        bad.method = "POST".to_owned();
        bad.body = Some("{{ secret.TOKEN }}".to_owned());
        assert_eq!(
            fields(&bad),
            [("request.body".to_owned(), ViolationKind::SecretNotAllowed)]
        );
    }

    #[test]
    fn managed_duplicate_and_malformed_headers_are_refused() {
        let mut bad = request();
        for name in ["Host", "X-Webhooks-Live-Hop", "X-One", "x-one", "bad name"] {
            bad.headers.push(ScheduleHeader {
                name: name.to_owned(),
                value: "v".to_owned(),
            });
        }
        let found = fields(&bad);
        let flagged: Vec<&str> = found.iter().map(|(field, _)| field.as_str()).collect();
        assert_eq!(
            flagged,
            [
                "request.headers.0.name",
                "request.headers.1.name",
                "request.headers.3.name",
                "request.headers.4.name"
            ]
        );
    }

    #[test]
    fn the_method_and_size_limits_are_enforced() {
        let mut bad = request();
        bad.method = "TRACE".to_owned();
        bad.url = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
        bad.body = Some("x".repeat(MAX_BODY_BYTES + 1));
        assert_eq!(
            fields(&bad),
            [
                ("request.method".to_owned(), ViolationKind::InvalidFormat),
                ("request.url".to_owned(), ViolationKind::TooLong),
                ("request.body".to_owned(), ViolationKind::TooLong),
            ]
        );
    }
}
