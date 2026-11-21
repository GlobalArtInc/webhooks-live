use std::collections::BTreeSet;

use regex::{Regex, RegexBuilder};
use serde_json::{Map, Value};
use uuid::Uuid;
use whl_script::ScriptCompiler;
use whl_template::{
    ErrorCode, ParseLimits, Policy, RenderTarget, SecretPolicy, StepPolicy, Template,
};

use crate::error::{MAX_DIAGNOSTICS, RuleDiagnostic, RuleError, RuleErrorCode};
use crate::limits::{MAX_PATH_BYTES, MAX_REGEX_SIZE_BYTES, MAX_VARIABLE_NAME_BYTES};
use crate::set::References;

pub(crate) struct Checker<'a> {
    pub(crate) references: &'a References,
    pub(crate) scripts: &'a ScriptCompiler,
    diagnostics: Vec<RuleDiagnostic>,
}

pub(crate) fn join(path: &str, key: &str) -> String {
    format!("{path}.{key}")
}

pub(crate) fn compile_regex(pattern: &str) -> Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE_BYTES)
        .build()
}

pub(crate) fn is_variable_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() <= MAX_VARIABLE_NAME_BYTES
        && bytes.split_first().is_some_and(|(first, rest)| {
            (first.is_ascii_lowercase() || *first == b'_')
                && rest
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        })
}

pub(crate) fn is_step_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() <= crate::limits::MAX_STEP_NAME_BYTES
        && bytes.split_first().is_some_and(|(first, rest)| {
            first.is_ascii_lowercase()
                && rest
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        })
}

fn is_path_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'[' | b']')
}

fn template_code(code: ErrorCode) -> RuleErrorCode {
    match code {
        ErrorCode::SourceTooLarge | ErrorCode::LiteralTooLarge | ErrorCode::OutputTooLarge => {
            RuleErrorCode::TooLong
        }
        ErrorCode::TooManyExpressions | ErrorCode::TooManyFilters | ErrorCode::PathTooDeep => {
            RuleErrorCode::TooMany
        }
        ErrorCode::SecretNotAllowed => RuleErrorCode::NotAllowed,
        _ => RuleErrorCode::InvalidFormat,
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn new(references: &'a References, scripts: &'a ScriptCompiler) -> Self {
        Self {
            references,
            scripts,
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.diagnostics.len()
    }

    pub(crate) fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub(crate) fn fail(
        &mut self,
        field: impl Into<String>,
        code: RuleErrorCode,
        message: impl Into<String>,
    ) {
        self.diagnostics
            .push(RuleDiagnostic::new(field, code, message));
    }

    pub(crate) fn finish(self) -> Result<(), RuleError> {
        if self.diagnostics.is_empty() {
            return Ok(());
        }
        let truncated = self.diagnostics.len() > MAX_DIAGNOSTICS;
        let mut diagnostics = self.diagnostics;
        diagnostics.truncate(MAX_DIAGNOSTICS);
        Err(RuleError {
            diagnostics,
            truncated,
        })
    }

    pub(crate) fn object<'v>(
        &mut self,
        value: &'v Value,
        path: &str,
    ) -> Option<&'v Map<String, Value>> {
        if let Value::Object(map) = value {
            Some(map)
        } else {
            self.fail(path, RuleErrorCode::InvalidFormat, "must be a JSON object");
            None
        }
    }

    pub(crate) fn known(&mut self, map: &Map<String, Value>, path: &str, allowed: &[&str]) {
        let unknown: Vec<String> = map
            .keys()
            .filter(|key| !allowed.contains(&key.as_str()))
            .cloned()
            .collect();
        for key in unknown {
            self.fail(
                join(path, &key),
                RuleErrorCode::NotAllowed,
                "is not a known field",
            );
        }
    }

    pub(crate) fn require(&mut self, map: &Map<String, Value>, path: &str, key: &str) -> bool {
        let present = map.get(key).is_some_and(|value| !value.is_null());
        if !present {
            self.fail(join(path, key), RuleErrorCode::Required, "is required");
        }
        present
    }

    pub(crate) fn text<'v>(
        &mut self,
        map: &'v Map<String, Value>,
        path: &str,
        key: &str,
    ) -> Option<&'v str> {
        match map.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text),
            Some(_) => {
                self.fail(
                    join(path, key),
                    RuleErrorCode::InvalidFormat,
                    "must be a string",
                );
                None
            }
        }
    }

    pub(crate) fn required_text<'v>(
        &mut self,
        map: &'v Map<String, Value>,
        path: &str,
        key: &str,
    ) -> Option<&'v str> {
        if !self.require(map, path, key) {
            return None;
        }
        self.text(map, path, key)
    }

    pub(crate) fn boolean(
        &mut self,
        map: &Map<String, Value>,
        path: &str,
        key: &str,
        default: bool,
    ) -> bool {
        match map.get(key) {
            None | Some(Value::Null) => default,
            Some(Value::Bool(flag)) => *flag,
            Some(_) => {
                self.fail(
                    join(path, key),
                    RuleErrorCode::InvalidFormat,
                    "must be a boolean",
                );
                default
            }
        }
    }

    pub(crate) fn array<'v>(
        &mut self,
        map: &'v Map<String, Value>,
        path: &str,
        key: &str,
    ) -> Option<&'v Vec<Value>> {
        match map.get(key) {
            None | Some(Value::Null) => None,
            Some(Value::Array(items)) => Some(items),
            Some(_) => {
                self.fail(
                    join(path, key),
                    RuleErrorCode::InvalidFormat,
                    "must be an array",
                );
                None
            }
        }
    }

    pub(crate) fn uuid(&mut self, map: &Map<String, Value>, path: &str, key: &str) -> Option<Uuid> {
        if !self.require(map, path, key) {
            return None;
        }
        self.optional_uuid(map, path, key)
    }

    pub(crate) fn optional_uuid(
        &mut self,
        map: &Map<String, Value>,
        path: &str,
        key: &str,
    ) -> Option<Uuid> {
        let text = self.text(map, path, key)?;
        let parsed = Uuid::try_parse(text)
            .ok()
            .filter(|uuid| uuid.hyphenated().to_string() == text);
        if parsed.is_none() {
            self.fail(
                join(path, key),
                RuleErrorCode::InvalidFormat,
                "must be a lowercase hyphenated UUID",
            );
        }
        parsed
    }

    pub(crate) fn template(
        &mut self,
        field: &str,
        source: &str,
        target: RenderTarget,
        secrets: SecretPolicy,
        steps: &BTreeSet<String>,
    ) -> bool {
        let before = self.count();
        let policy = Policy {
            target,
            secrets,
            steps: StepPolicy::Only(steps.clone()),
            limits: ParseLimits::default(),
        };
        match Template::parse(source, &policy) {
            Err(error) => {
                for diagnostic in error.diagnostics {
                    let message = format!("{}: {}", diagnostic.code.as_str(), diagnostic.message);
                    self.diagnostics.push(
                        RuleDiagnostic::new(field, template_code(diagnostic.code), message)
                            .with_position(diagnostic.position.into()),
                    );
                }
            }
            Ok(template) => {
                for name in template.referenced_secrets() {
                    if !self.references.secrets.contains(&name) {
                        self.fail(
                            field,
                            RuleErrorCode::InvalidFormat,
                            format!("secret {name} is not defined on this endpoint"),
                        );
                    }
                }
            }
        }
        self.count() == before
    }

    pub(crate) fn json_path(&mut self, field: &str, path: &str) {
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        let source = format!("{{{{ json(\"{escaped}\") }}}}");
        if let Err(error) = Template::parse(&source, &Policy::response(RenderTarget::Body)) {
            for diagnostic in error.diagnostics {
                let message = format!("invalid JSON path: {}", diagnostic.message);
                self.fail(field, RuleErrorCode::InvalidFormat, message);
            }
        }
    }

    pub(crate) fn variable_path(
        &mut self,
        field: &str,
        path: &str,
        steps: &BTreeSet<String>,
    ) -> bool {
        const MESSAGE: &str = "must be a variable path such as request.header.x-token";
        if path.is_empty() || path.len() > MAX_PATH_BYTES || !path.bytes().all(is_path_byte) {
            self.fail(field, RuleErrorCode::InvalidFormat, MESSAGE);
            return false;
        }
        let policy = Policy {
            target: RenderTarget::Body,
            secrets: SecretPolicy::Forbidden,
            steps: StepPolicy::Only(steps.clone()),
            limits: ParseLimits::default(),
        };
        match Template::parse(&format!("{{{{ {path} }}}}"), &policy) {
            Ok(_) => true,
            Err(_) => {
                self.fail(field, RuleErrorCode::InvalidFormat, MESSAGE);
                false
            }
        }
    }

    pub(crate) fn script(&mut self, field: &str, source: &str) {
        if let Err(error) = self.scripts.compile(source) {
            for diagnostic in error.diagnostics {
                let message = format!("{}: {}", diagnostic.code.as_str(), diagnostic.message);
                self.diagnostics.push(
                    RuleDiagnostic::new(field, RuleErrorCode::InvalidFormat, message)
                        .with_position(diagnostic.position.into()),
                );
            }
        }
    }
}
