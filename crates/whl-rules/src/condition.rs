use std::collections::BTreeSet;

use ipnet::IpNet;
use serde_json::Value;

use crate::check::{Checker, join};
use crate::error::RuleErrorCode;
use crate::limits::{
    MAX_ARGUMENT_BYTES, MAX_CONDITION_DEPTH, MAX_CONDITION_LEAVES, MAX_CONDITION_LIST_ITEMS,
};
use crate::model::{Argument, Condition, Leaf, Op};

pub(crate) fn validate_condition(
    checker: &mut Checker,
    value: &Value,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Condition> {
    let mut leaves = 0;
    let condition = node(checker, value, path, steps, 1, &mut leaves);
    if leaves > MAX_CONDITION_LEAVES {
        checker.fail(
            path,
            RuleErrorCode::TooMany,
            format!("must have at most {MAX_CONDITION_LEAVES} leaves"),
        );
        return None;
    }
    condition
}

fn node(
    checker: &mut Checker,
    value: &Value,
    path: &str,
    steps: &BTreeSet<String>,
    depth: usize,
    leaves: &mut usize,
) -> Option<Condition> {
    if depth > MAX_CONDITION_DEPTH {
        checker.fail(
            path,
            RuleErrorCode::TooMany,
            format!("must nest at most {MAX_CONDITION_DEPTH} levels"),
        );
        return None;
    }
    let map = checker.object(value, path)?;
    if map.is_empty() {
        return Some(Condition::Always);
    }
    let keys: Vec<&str> = ["all", "any", "not", "value"]
        .into_iter()
        .filter(|key| map.contains_key(*key))
        .collect();
    match keys.as_slice() {
        [] => {
            checker.fail(
                path,
                RuleErrorCode::InvalidFormat,
                "must contain one of all, any, not or value",
            );
            None
        }
        [key] => {
            let key = *key;
            if key == "value" {
                checker.known(map, path, &["value", "op", "arg"]);
                *leaves += 1;
                return leaf(checker, map, path, steps).map(Condition::Leaf);
            }
            checker.known(map, path, &[key]);
            combinator(checker, map, path, key, steps, depth, leaves)
        }
        _ => {
            checker.fail(
                path,
                RuleErrorCode::InvalidFormat,
                "must contain only one of all, any, not or value",
            );
            None
        }
    }
}

fn combinator(
    checker: &mut Checker,
    map: &serde_json::Map<String, Value>,
    path: &str,
    key: &str,
    steps: &BTreeSet<String>,
    depth: usize,
    leaves: &mut usize,
) -> Option<Condition> {
    let field = join(path, key);
    let item = map.get(key)?;
    if key == "not" {
        return node(checker, item, &field, steps, depth + 1, leaves)
            .map(|inner| Condition::Not(Box::new(inner)));
    }
    let Value::Array(items) = item else {
        checker.fail(field, RuleErrorCode::InvalidFormat, "must be an array");
        return None;
    };
    if items.is_empty() {
        checker.fail(
            field,
            RuleErrorCode::Required,
            "must have at least one condition",
        );
        return None;
    }
    let mut children = Vec::with_capacity(items.len());
    for (index, child) in items.iter().enumerate() {
        let child_path = format!("{field}.{index}");
        if let Some(condition) = node(checker, child, &child_path, steps, depth + 1, leaves) {
            children.push(condition);
        }
    }
    if children.len() != items.len() {
        return None;
    }
    Some(if key == "all" {
        Condition::All(children)
    } else {
        Condition::Any(children)
    })
}

fn leaf(
    checker: &mut Checker,
    map: &serde_json::Map<String, Value>,
    path: &str,
    steps: &BTreeSet<String>,
) -> Option<Leaf> {
    let variable = checker.required_text(map, path, "value");
    let op_text = checker.required_text(map, path, "op")?;
    let variable = variable?;
    let op = if let Some(op) = Op::parse(op_text) {
        op
    } else {
        checker.fail(
            join(path, "op"),
            RuleErrorCode::InvalidFormat,
            "is not a known operator",
        );
        return None;
    };
    if !checker.variable_path(&join(path, "value"), variable, steps) {
        return None;
    }
    let argument = argument(checker, map, path, op)?;
    Some(Leaf {
        path: variable.to_owned(),
        op,
        argument,
    })
}

fn argument(
    checker: &mut Checker,
    map: &serde_json::Map<String, Value>,
    path: &str,
    op: Op,
) -> Option<Option<Argument>> {
    let field = join(path, "arg");
    let value = map.get("arg").filter(|value| !value.is_null());
    match (op, value) {
        (Op::Exists, None) => Some(None),
        (Op::Exists, Some(_)) => {
            checker.fail(
                field,
                RuleErrorCode::NotAllowed,
                "must be absent for exists",
            );
            None
        }
        (_, None) => {
            checker.fail(field, RuleErrorCode::Required, "is required");
            None
        }
        (Op::Gt | Op::Lt | Op::Eq | Op::Ne, Some(Value::Number(number))) => {
            Some(Some(Argument::Number(number.clone())))
        }
        (Op::Gt | Op::Lt, Some(_)) => {
            checker.fail(field, RuleErrorCode::InvalidFormat, "must be a number");
            None
        }
        (Op::In, Some(Value::Array(items))) => {
            list(checker, &field, items, false).map(|items| Some(Argument::List(items)))
        }
        (Op::InCidr, Some(Value::Array(items))) => {
            list(checker, &field, items, true).map(|items| Some(Argument::List(items)))
        }
        (Op::In | Op::InCidr, Some(_)) => {
            checker.fail(
                field,
                RuleErrorCode::InvalidFormat,
                "must be a list of strings",
            );
            None
        }
        (Op::Matches, Some(Value::String(pattern))) => {
            if !checker.pattern(&field, pattern) {
                return None;
            }
            Some(Some(Argument::Text(pattern.clone())))
        }
        (Op::Matches, Some(_)) => {
            checker.fail(
                field,
                RuleErrorCode::InvalidFormat,
                "must be a regular expression",
            );
            None
        }
        (_, Some(Value::String(text))) => {
            if text.len() > MAX_ARGUMENT_BYTES {
                checker.fail(
                    field,
                    RuleErrorCode::TooLong,
                    format!("must be at most {MAX_ARGUMENT_BYTES} bytes"),
                );
                return None;
            }
            Some(Some(Argument::Text(text.clone())))
        }
        (_, Some(_)) => {
            checker.fail(
                field,
                RuleErrorCode::InvalidFormat,
                "must be a string or number",
            );
            None
        }
    }
}

fn list(checker: &mut Checker, field: &str, items: &[Value], cidr: bool) -> Option<Vec<String>> {
    if items.is_empty() {
        checker.fail(field, RuleErrorCode::Required, "must not be empty");
        return None;
    }
    if items.len() > MAX_CONDITION_LIST_ITEMS {
        checker.fail(
            field,
            RuleErrorCode::TooMany,
            format!("must have at most {MAX_CONDITION_LIST_ITEMS} entries"),
        );
        return None;
    }
    let mut list = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::String(text) if !cidr || text.parse::<IpNet>().is_ok() => {
                list.push(text.clone());
            }
            _ => {
                let message = if cidr {
                    "must contain only CIDR ranges"
                } else {
                    "must contain only strings"
                };
                checker.fail(field, RuleErrorCode::InvalidFormat, message);
                return None;
            }
        }
    }
    Some(list)
}
