use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;
use time::OffsetDateTime;
use whl_auth::{Caller, Creation};
use whl_core::endpoint_id::EndpointId;
use whl_rules::{References, validate_set};
use whl_script::{EngineLimits, ScriptCompiler};
use whl_secrets::SecretName;
use whl_store::forward::NewForwardRule;
use whl_store::schedules::NewSchedule;
use whl_store::{
    AccessSettings, ClonedForwardRule, EndpointChange, EndpointParts, Lifecycle, Uuid,
};

use super::document::ConfigDocument;
use super::export::Selection;
use crate::access;
use crate::error::{ApiError, FieldCode, FieldViolation};
use crate::forward::build_rule;
use crate::input::{AccessPatch, EndpointPatch, ParsedInput, ResponsePatch, parse_endpoint_input};
use crate::lifecycle::{self, Standing, combine};
use crate::notify::build_channel;
use crate::resolve::{defaults, resolve};
use crate::rules::{rule_violations, vet_all};
use crate::schedules::{ParsedSchedule, check_cron, check_request_with, parse_schedule};
use crate::state::ApiState;

const DEFAULT_TIMEZONE: &str = "UTC";
const FORWARD_RULE_ID: &str = "forward_rule_id";
const CHANNEL_ID: &str = "channel_id";
const NOTIFY_KIND: &str = "notify";

pub struct Overrides {
    pub patch: EndpointPatch,
    pub violations: Vec<FieldViolation>,
}

impl Overrides {
    pub fn none() -> Self {
        Self {
            patch: EndpointPatch::default(),
            violations: Vec::new(),
        }
    }

    pub fn from_input(parsed: ParsedInput) -> Self {
        Self {
            patch: parsed.patch,
            violations: parsed.violations,
        }
    }
}

pub struct Prepared {
    pub id: EndpointId,
    pub change: EndpointChange,
    pub parts: EndpointParts,
    pub secrets_missing: Vec<String>,
    pub skipped: Vec<String>,
}

struct Work<'a> {
    state: &'a ApiState,
    caller: &'a Caller,
    creation: &'a Creation,
    document: &'a ConfigDocument,
    selection: Selection,
    id: EndpointId,
    violations: Vec<FieldViolation>,
    skipped: Vec<String>,
    missing: BTreeSet<String>,
    parts: EndpointParts,
}

pub async fn prepare(
    state: &ApiState,
    caller: &Caller,
    creation: &Creation,
    document: &ConfigDocument,
    selection: Selection,
    overrides: Overrides,
) -> Result<Prepared, ApiError> {
    let id = EndpointId::generate().map_err(|_| ApiError::EntropyUnavailable)?;
    let mut work = Work {
        state,
        caller,
        creation,
        document,
        selection,
        id: id.clone(),
        violations: Vec::new(),
        skipped: Vec::new(),
        missing: document.secret_names.clone(),
        parts: EndpointParts::default(),
    };
    let change = work.endpoint(overrides)?;
    let forward_ids = work.forward_rules().await?;
    let channel_ids = work.channels().await?;
    work.rules(&forward_ids, &channel_ids).await?;
    work.schedules().await?;
    if !work.violations.is_empty() {
        return Err(ApiError::Validation(work.violations));
    }
    let change = change.ok_or(ApiError::InternalFault(
        "the endpoint settings were not resolved",
    ))?;
    Ok(Prepared {
        id,
        change,
        parts: work.parts,
        secrets_missing: work.missing.into_iter().collect(),
        skipped: work.skipped,
    })
}

impl Work<'_> {
    fn absorb<T>(
        &mut self,
        result: Result<T, ApiError>,
        prefix: &str,
    ) -> Result<Option<T>, ApiError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(ApiError::Validation(found)) => {
                self.violations.extend(found.into_iter().map(|violation| {
                    if prefix.is_empty() {
                        violation
                    } else {
                        violation.within(prefix)
                    }
                }));
                Ok(None)
            }
            Err(other) => Err(other),
        }
    }

    fn endpoint(&mut self, overrides: Overrides) -> Result<Option<EndpointChange>, ApiError> {
        let Overrides {
            patch: over,
            violations: mut found,
        } = overrides;
        let mut patch = if self.selection.endpoint {
            let bytes = serde_json::to_vec(&Value::Object(self.document.endpoint.clone()))
                .map_err(|_| {
                    ApiError::InternalFault("the endpoint document could not be encoded")
                })?;
            let parsed = parse_endpoint_input(&bytes, true)?;
            found.extend(
                parsed
                    .violations
                    .into_iter()
                    .map(|violation| violation.within("endpoint")),
            );
            parsed.patch
        } else {
            EndpointPatch::default()
        };
        patch = overlay(patch, over);
        if patch.view_password.take().is_some() {
            found.push(FieldViolation::new(
                "view_password",
                FieldCode::NotAllowed,
                "cannot be set when an endpoint is created from a document",
            ));
        }
        let mut verifier_dropped = false;
        match patch.verifier.take() {
            Some(Some(_)) if !self.state.secrets_enabled() => {
                self.skipped.push("verifier".to_owned());
            }
            Some(Some(input)) => {
                if let Ok(name) = SecretName::parse(&input.secret_name) {
                    self.missing.insert(name.unpack().to_owned());
                }
                patch.verifier = Some(Some(input));
                verifier_dropped = true;
            }
            other => patch.verifier = other,
        }
        let standing = Standing {
            owned: matches!(self.creation, Creation::Owned(_)),
            signed_in: self.caller.user().is_some(),
            sign_in_possible: self.state.access.has_users(),
            role: None,
        };
        let combined = combine(
            resolve(&self.state.settings, &defaults(), &patch, found),
            lifecycle::resolve(
                &self.state.settings,
                self.state.creation_tier(self.caller, self.creation),
                standing,
                &Lifecycle::default(),
                &patch.lifecycle,
            ),
            access::resolve(
                self.state.secrets.as_deref(),
                standing,
                &self.id,
                &AccessSettings::default(),
                &patch.access,
            ),
        );
        let Some(mut change) = self.absorb(combined, "")? else {
            return Ok(None);
        };
        if verifier_dropped {
            change.settings.verifier = None;
            self.skipped.push("verifier".to_owned());
        }
        Ok(Some(change))
    }

    async fn forward_rules(&mut self) -> Result<BTreeMap<Uuid, Uuid>, ApiError> {
        let mut ids = BTreeMap::new();
        let document = self.document;
        if !self.selection.forward_rules || document.forward_rules.is_empty() {
            return Ok(ids);
        }
        if !self.state.settings.forwarding {
            self.skipped.push("forward_rules".to_owned());
            return Ok(ids);
        }
        let mut seen = BTreeSet::new();
        for (index, value) in document.forward_rules.iter().enumerate() {
            let prefix = format!("forward_rules.{index}");
            let mut object = value.as_object().cloned().unwrap_or_default();
            let old = match object.remove("id") {
                None | Some(Value::Null) => None,
                Some(Value::String(text)) => match Uuid::try_parse(&text) {
                    Ok(old) if seen.insert(old) => Some(old),
                    Ok(_) => {
                        self.violations.push(FieldViolation::new(
                            format!("{prefix}.id"),
                            FieldCode::Duplicate,
                            "is listed twice",
                        ));
                        continue;
                    }
                    Err(_) => {
                        self.violations.push(FieldViolation::new(
                            format!("{prefix}.id"),
                            FieldCode::InvalidFormat,
                            "must be a UUID",
                        ));
                        continue;
                    }
                },
                Some(_) => {
                    self.violations.push(FieldViolation::new(
                        format!("{prefix}.id"),
                        FieldCode::InvalidFormat,
                        "must be a UUID",
                    ));
                    continue;
                }
            };
            let bytes = encode(&Value::Object(object))?;
            let built: Result<NewForwardRule, ApiError> = build_rule(self.state, &bytes).await;
            if let Some(rule) = self.absorb(built, &prefix)? {
                let new = Uuid::now_v7();
                if let Some(old) = old {
                    ids.insert(old, new);
                }
                self.parts
                    .forward_rules
                    .push(ClonedForwardRule { id: new, rule });
            }
        }
        Ok(ids)
    }

    async fn rules(
        &mut self,
        forward_ids: &BTreeMap<Uuid, Uuid>,
        channel_ids: &BTreeMap<Uuid, Uuid>,
    ) -> Result<(), ApiError> {
        let document = self.document;
        if !self.selection.rules || document.rules.is_empty() {
            return Ok(());
        }
        if !self.selection.forward_rules
            && document
                .rules
                .iter()
                .any(|rule| uses_step_field(rule, FORWARD_RULE_ID))
        {
            self.violations.push(FieldViolation::new(
                "include",
                FieldCode::NotAllowed,
                "rules that forward through a forward rule need forward_rules in include",
            ));
            return Ok(());
        }
        let mut inputs: Vec<Value> = Vec::with_capacity(document.rules.len());
        for (index, rule) in document.rules.iter().enumerate() {
            match remap_references(rule, forward_ids, channel_ids) {
                Some(remapped) => inputs.push(remapped),
                None => self.skipped.push(format!("rules.{index}")),
            }
        }
        if inputs.is_empty() {
            return Ok(());
        }
        let references = References {
            secrets: document.secret_names.clone(),
            forward_rules: self
                .parts
                .forward_rules
                .iter()
                .map(|cloned| cloned.id)
                .collect(),
            channels: self
                .parts
                .channels
                .iter()
                .map(|channel| channel.id)
                .collect(),
        };
        let scripts = ScriptCompiler::new(EngineLimits::default()).map_err(ApiError::RuleEngine)?;
        match validate_set(&inputs, &references, &scripts) {
            Ok(rules) => {
                let vetted = vet_all(self.state, &rules).await;
                if self.absorb(vetted, "")?.is_some() {
                    self.parts.rules = rules;
                }
            }
            Err(error) => self
                .violations
                .extend(rule_violations(error.diagnostics, None)),
        }
        Ok(())
    }

    async fn channels(&mut self) -> Result<BTreeMap<Uuid, Uuid>, ApiError> {
        let mut ids = BTreeMap::new();
        let document = self.document;
        if !self.selection.channels || document.channels.is_empty() {
            return Ok(ids);
        }
        if !self.state.notifications.enabled || !self.state.secrets_enabled() {
            self.skipped.push("channels".to_owned());
            return Ok(ids);
        }
        let created_by = self.caller.user().map(|user| user.id);
        for (index, value) in document.channels.iter().enumerate() {
            let prefix = format!("channels.{index}");
            let mut object = value.as_object().cloned().unwrap_or_default();
            let old = object
                .remove("id")
                .and_then(|id| id.as_str().and_then(|text| Uuid::try_parse(text).ok()));
            let kind = object.get("kind").and_then(Value::as_str);
            let config = object.get("config");
            if kind == Some("telegram") {
                if let Some(name) = config
                    .and_then(|config| config.get("token_secret"))
                    .and_then(Value::as_str)
                    .and_then(|text| SecretName::parse(text).ok())
                {
                    self.missing.insert(name.unpack().to_owned());
                }
                self.skipped.push(prefix);
                continue;
            }
            if config.is_none() {
                self.skipped.push(prefix);
                continue;
            }
            let bytes = encode(&Value::Object(object))?;
            let new = Uuid::now_v7();
            let built = build_channel(
                self.state,
                self.caller,
                &self.id,
                new,
                self.state.creation_tier(self.caller, self.creation),
                &bytes,
            )
            .await;
            if let Some(mut channel) = self.absorb(built, &prefix)? {
                channel.created_by = created_by;
                if let Some(old) = old {
                    ids.insert(old, new);
                }
                self.parts.channels.push(channel);
            }
        }
        Ok(ids)
    }

    async fn schedules(&mut self) -> Result<(), ApiError> {
        let document = self.document;
        if !self.selection.schedules || document.schedules.is_empty() {
            return Ok(());
        }
        let Some(user) = self.caller.user().map(|user| user.id) else {
            self.skipped.push("schedules".to_owned());
            return Ok(());
        };
        if !self.state.schedules.enabled || !self.state.access.has_users() {
            self.skipped.push("schedules".to_owned());
            return Ok(());
        }
        let now = OffsetDateTime::now_utc();
        for (index, value) in document.schedules.iter().enumerate() {
            let prefix = format!("schedules.{index}");
            let bytes = encode(value)?;
            let ParsedSchedule {
                name,
                cron,
                timezone,
                request,
                violations: mut found,
                ..
            } = parse_schedule(&bytes, true)?;
            let timezone = timezone.unwrap_or_else(|| DEFAULT_TIMEZONE.to_owned());
            if let Some(cron) = &cron {
                let _ = check_cron(
                    self.state
                        .creation_tier(self.caller, self.creation)
                        .limits()
                        .schedule_min_interval,
                    cron,
                    &timezone,
                    now,
                    &mut found,
                );
            }
            if let Some(request) = &request {
                check_request_with(self.state, &self.document.secret_names, request, &mut found)
                    .await;
            }
            match (name, cron, request) {
                (Some(name), Some(cron), Some(request)) if found.is_empty() => {
                    self.parts.schedules.push(NewSchedule {
                        created_by: user,
                        name,
                        cron,
                        timezone,
                        request,
                        enabled: false,
                        next_run_at: None,
                    });
                }
                _ => self
                    .violations
                    .extend(found.into_iter().map(|violation| violation.within(&prefix))),
            }
        }
        Ok(())
    }
}

fn encode(value: &Value) -> Result<Vec<u8>, ApiError> {
    serde_json::to_vec(value)
        .map_err(|_| ApiError::InternalFault("a configuration part could not be encoded"))
}

fn uses_step_field(rule: &Value, field: &str) -> bool {
    rule.get("steps")
        .and_then(Value::as_array)
        .is_some_and(|steps| steps.iter().any(|step| step.get(field).is_some()))
}

fn remap_references(
    rule: &Value,
    forward_ids: &BTreeMap<Uuid, Uuid>,
    channel_ids: &BTreeMap<Uuid, Uuid>,
) -> Option<Value> {
    let mut rule = rule.clone();
    let steps = rule
        .get_mut("steps")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten();
    for step in steps {
        let Some(object) = step.as_object_mut() else {
            continue;
        };
        if let Some(new) = mapped(object, FORWARD_RULE_ID, forward_ids) {
            object.insert(FORWARD_RULE_ID.to_owned(), new);
        }
        if object.get("kind").and_then(Value::as_str) == Some(NOTIFY_KIND) {
            let new = mapped(object, CHANNEL_ID, channel_ids)?;
            object.insert(CHANNEL_ID.to_owned(), new);
        }
    }
    Some(rule)
}

fn mapped(
    object: &serde_json::Map<String, Value>,
    key: &str,
    ids: &BTreeMap<Uuid, Uuid>,
) -> Option<Value> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|text| Uuid::try_parse(text).ok())
        .and_then(|old| ids.get(&old))
        .map(|new| Value::String(new.hyphenated().to_string()))
}

pub fn overlay(base: EndpointPatch, over: EndpointPatch) -> EndpointPatch {
    EndpointPatch {
        title: over.title.or(base.title),
        response: ResponsePatch {
            status: over.response.status.or(base.response.status),
            content_type: over.response.content_type.or(base.response.content_type),
            headers: over.response.headers.or(base.response.headers),
            body: over.response.body.or(base.response.body),
            delay_ms: over.response.delay_ms.or(base.response.delay_ms),
            templated: over.response.templated.or(base.response.templated),
            status_template: over
                .response
                .status_template
                .or(base.response.status_template),
        },
        cors: over.cors.or(base.cors),
        overflow: over.overflow.or(base.overflow),
        tags: over.tags.or(base.tags),
        lifecycle: crate::input::LifecyclePatch {
            retention: over.lifecycle.retention.or(base.lifecycle.retention),
            max_captures: over.lifecycle.max_captures.or(base.lifecycle.max_captures),
            capture_ttl_seconds: over
                .lifecycle
                .capture_ttl_seconds
                .or(base.lifecycle.capture_ttl_seconds),
        },
        verifier: over.verifier.or(base.verifier),
        access: AccessPatch {
            sender_allow: over.access.sender_allow.or(base.access.sender_allow),
            capture_auth: over.access.capture_auth.or(base.access.capture_auth),
        },
        view_password: over.view_password.or(base.view_password),
    }
}
