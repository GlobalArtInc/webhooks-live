use std::collections::BTreeSet;

use serde_json::Value;
use uuid::Uuid;
use whl_script::ScriptCompiler;

use crate::check::Checker;
use crate::condition::validate_condition;
use crate::error::{RuleError, RuleErrorCode};
use crate::kind::StepKind;
use crate::limits::{MAX_RULE_NAME_CHARS, MAX_RULE_SET_BYTES, RuleLimits};
use crate::model::{Action, Condition, Rule};
use crate::plan::RulePlan;
use crate::step::validate_step;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct References {
    pub secrets: BTreeSet<String>,
    pub forward_rules: BTreeSet<Uuid>,
    pub channels: BTreeSet<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundTarget {
    pub field: String,
    pub url: String,
}

pub fn validate_set(
    rules: &[Value],
    references: &References,
    scripts: &ScriptCompiler,
) -> Result<Vec<Rule>, RuleError> {
    validate_set_with(rules, references, scripts, RuleLimits::default())
}

pub const HTTP_BEFORE_CUT_MESSAGE: &str =
    "an http step may stand only after the last response step";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StepSlot {
    pub rule: usize,
    pub step: usize,
}

pub fn validate_set_with(
    rules: &[Value],
    references: &References,
    scripts: &ScriptCompiler,
    limits: RuleLimits,
) -> Result<Vec<Rule>, RuleError> {
    check_set(Checker::new(references, scripts), rules, limits, true)
}

pub fn validate_set_shape(
    rules: &[Value],
    references: &References,
    scripts: &ScriptCompiler,
    limits: RuleLimits,
) -> Result<Vec<Rule>, RuleError> {
    check_set(Checker::new(references, scripts), rules, limits, false)
}

pub fn http_before_cut(rules: &[Rule]) -> Vec<StepSlot> {
    let enabled: Vec<(usize, &Rule)> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule.enabled)
        .collect();
    let plan = RulePlan::of(
        &enabled
            .iter()
            .map(|(_, rule)| (*rule).clone())
            .collect::<Vec<_>>(),
    );
    let mut slots = Vec::new();
    for (position, (rule_index, rule)) in enabled.iter().enumerate() {
        for (step_index, step) in rule.steps.iter().enumerate() {
            if StepKind::of(&step.action) == StepKind::Http && plan.is_before(position, step_index)
            {
                slots.push(StepSlot {
                    rule: *rule_index,
                    step: step_index,
                });
            }
        }
    }
    slots
}

pub fn load_set(rules: &[Value], scripts: &ScriptCompiler) -> Result<Vec<Rule>, RuleError> {
    let references = References::default();
    check_set(
        Checker::stored(&references, scripts),
        rules,
        RuleLimits::UNBOUNDED,
        false,
    )
}

fn check_set(
    mut checker: Checker<'_>,
    rules: &[Value],
    limits: RuleLimits,
    enforce_plan: bool,
) -> Result<Vec<Rule>, RuleError> {
    if rules.len() > limits.max_rules {
        checker.fail(
            "rules",
            RuleErrorCode::TooMany,
            format!("must have at most {} rules", limits.max_rules),
        );
        return checker.finish().map(|()| Vec::new());
    }
    let mut steps = BTreeSet::new();
    let mut valid = Vec::with_capacity(rules.len());
    let mut failed = false;
    for (index, value) in rules.iter().enumerate() {
        match validate_rule(
            &mut checker,
            value,
            &format!("rules.{index}"),
            &mut steps,
            limits.steps.covers(index).then_some(limits.max_steps),
        ) {
            Some(rule) => valid.push(rule),
            None => failed = true,
        }
    }
    if failed && checker.is_clean() {
        checker.fail(
            "rules",
            RuleErrorCode::InvalidFormat,
            "contains an invalid rule",
        );
    }
    if checker.is_clean() && enforce_plan {
        refuse_http_before_cut(&mut checker, &valid);
    }
    if checker.is_clean() {
        let bytes = valid
            .iter()
            .map(|rule| serde_json::to_vec(rule).map_or(usize::MAX, |json| json.len()))
            .fold(0usize, usize::saturating_add);
        if bytes > MAX_RULE_SET_BYTES {
            checker.fail(
                "rules",
                RuleErrorCode::TooLong,
                format!("must serialize to at most {MAX_RULE_SET_BYTES} bytes"),
            );
        }
    }
    checker.finish().map(|()| valid)
}

fn refuse_http_before_cut(checker: &mut Checker<'_>, rules: &[Rule]) {
    for slot in http_before_cut(rules) {
        checker.fail(
            format!("rules.{}.steps.{}.kind", slot.rule, slot.step),
            RuleErrorCode::NotAllowed,
            HTTP_BEFORE_CUT_MESSAGE,
        );
    }
}

pub fn outbound_targets(rules: &[Rule]) -> Vec<OutboundTarget> {
    let mut targets = Vec::new();
    for (rule_index, rule) in rules.iter().enumerate() {
        for (step_index, step) in rule.steps.iter().enumerate() {
            let base = format!("rules.{rule_index}.steps.{step_index}");
            match &step.action {
                Action::Forward {
                    target_url: Some(url),
                    ..
                } => targets.push(OutboundTarget {
                    field: format!("{base}.target_url"),
                    url: url.clone(),
                }),
                Action::StoreS3 { bucket_url, .. } => targets.push(OutboundTarget {
                    field: format!("{base}.bucket_url"),
                    url: bucket_url.clone(),
                }),
                _ => {}
            }
        }
    }
    targets
}

fn validate_rule(
    checker: &mut Checker,
    value: &Value,
    path: &str,
    steps: &mut BTreeSet<String>,
    max_steps: Option<usize>,
) -> Option<Rule> {
    let before = checker.count();
    let map = checker.object(value, path)?;
    checker.known(map, path, &["name", "enabled", "condition", "steps"]);
    let name = rule_name(checker, map, path);
    let enabled = checker.boolean(map, path, "enabled", true);
    let condition = match map.get("condition") {
        None | Some(Value::Null) => Some(Condition::Always),
        Some(condition) => {
            validate_condition(checker, condition, &format!("{path}.condition"), steps)
        }
    };
    let items = match map.get("steps") {
        Some(Value::Array(items)) => items,
        Some(Value::Null) | None => {
            checker.fail(
                format!("{path}.steps"),
                RuleErrorCode::Required,
                "is required",
            );
            return None;
        }
        Some(_) => {
            checker.fail(
                format!("{path}.steps"),
                RuleErrorCode::InvalidFormat,
                "must be an array",
            );
            return None;
        }
    };
    if items.is_empty() {
        checker.fail(
            format!("{path}.steps"),
            RuleErrorCode::Required,
            "must have at least one step",
        );
        return None;
    }
    if let Some(max_steps) = max_steps.filter(|max_steps| items.len() > *max_steps) {
        checker.fail(
            format!("{path}.steps"),
            RuleErrorCode::TooMany,
            format!("must have at most {max_steps} steps"),
        );
        return None;
    }
    let mut built = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let step_path = format!("{path}.steps.{index}");
        if let Some(step) = validate_step(checker, item, &step_path, steps) {
            if let Some(name) = &step.name {
                steps.insert(name.clone());
            }
            built.push(step);
        }
    }
    if checker.count() != before {
        return None;
    }
    Some(Rule {
        name: name?,
        enabled,
        condition: condition?,
        steps: built,
    })
}

fn rule_name(
    checker: &mut Checker,
    map: &serde_json::Map<String, Value>,
    path: &str,
) -> Option<String> {
    let text = checker.required_text(map, path, "name")?;
    let field = format!("{path}.name");
    if text.is_empty() {
        checker.fail(field, RuleErrorCode::Required, "must not be empty");
        None
    } else if text.chars().count() > MAX_RULE_NAME_CHARS {
        checker.fail(
            field,
            RuleErrorCode::TooLong,
            format!("must be at most {MAX_RULE_NAME_CHARS} characters"),
        );
        None
    } else if text.chars().any(char::is_control) {
        checker.fail(
            field,
            RuleErrorCode::InvalidFormat,
            "must not contain control characters",
        );
        None
    } else {
        Some(text.to_owned())
    }
}
