use std::cmp::Ordering;
use std::net::IpAddr;

use whl_template::{
    ChaChaRandom, Context, RenderEnv, RenderErrorCode, RenderLimits, RenderOutcome, ResponseBudget,
    SystemClock, Template,
};

use crate::compiled::{CompiledCondition, CompiledLeaf, LeafArgument};
use crate::failure::{Failure, FailureCode};
use crate::model::Op as LeafOp;
use crate::pattern::Pattern;

pub(crate) struct Text {
    pub output: String,
    pub missing: bool,
}

pub(crate) struct RenderIssue {
    pub failure: Failure,
    pub only_forbidden: bool,
}

pub(crate) struct Evaluator<'a> {
    pub context: &'a Context,
    pub random: &'a mut ChaChaRandom,
    pub clock: &'a SystemClock,
    pub budget: &'a ResponseBudget,
}

impl Evaluator<'_> {
    pub(crate) fn render(&mut self, template: &Template) -> Result<Text, RenderIssue> {
        render(self.context, self.random, self.clock, template, self.budget)
    }

    pub(crate) fn charge(&self, pattern: &Pattern, haystack: &str) -> Result<(), RenderIssue> {
        charge_pattern(pattern, haystack, self.budget, self.clock)
    }

    pub(crate) fn condition(&mut self, condition: &CompiledCondition) -> Result<bool, RenderIssue> {
        match condition {
            CompiledCondition::Always => Ok(true),
            CompiledCondition::All(items) => {
                for item in items {
                    if !self.condition(item)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            CompiledCondition::Any(items) => {
                for item in items {
                    if self.condition(item)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            CompiledCondition::Not(inner) => Ok(!self.condition(inner)?),
            CompiledCondition::Leaf(leaf) => {
                let text = self.render(&leaf.template)?;
                if let (LeafOp::Matches, LeafArgument::Pattern(pattern), false) =
                    (leaf.op, &leaf.argument, text.missing)
                {
                    self.charge(pattern, &text.output)?;
                }
                Ok(compare(leaf, &text))
            }
        }
    }
}

pub(crate) fn charge_pattern(
    pattern: &Pattern,
    haystack: &str,
    budget: &ResponseBudget,
    clock: &SystemClock,
) -> Result<(), RenderIssue> {
    if pattern.cost(haystack.len()) > budget.remaining(clock) {
        return Err(RenderIssue {
            failure: Failure::new(
                FailureCode::Budget,
                "the regular expression would not finish inside the time budget",
            ),
            only_forbidden: false,
        });
    }
    Ok(())
}

pub(crate) fn render(
    context: &Context,
    random: &mut ChaChaRandom,
    clock: &SystemClock,
    template: &Template,
    budget: &ResponseBudget,
) -> Result<Text, RenderIssue> {
    let limits = RenderLimits::for_target(template.target());
    let mut env = RenderEnv::new(random, clock);
    match template.render_within(context, &limits, &mut env, budget) {
        RenderOutcome::Clean(rendered) => Ok(Text {
            missing: !rendered.missing.is_empty(),
            output: rendered.output,
        }),
        RenderOutcome::Partial(partial) => {
            let timed_out = partial.errors.iter().any(|error| {
                matches!(
                    error.code,
                    RenderErrorCode::BudgetExhausted | RenderErrorCode::DeadlineExceeded
                )
            });
            let only_forbidden = !partial.errors.is_empty()
                && partial
                    .errors
                    .iter()
                    .all(|error| error.code == RenderErrorCode::ForbiddenCharacter);
            let failure = if timed_out {
                Failure::new(FailureCode::Budget, "the render time budget is exhausted")
            } else {
                let detail = partial.errors.first().map_or_else(
                    || "the template did not render".to_owned(),
                    |error| {
                        format!(
                            "{} at line {}, column {}",
                            error.code.as_str(),
                            error.position.line,
                            error.position.column
                        )
                    },
                );
                Failure::new(FailureCode::Template, detail)
            };
            Err(RenderIssue {
                failure,
                only_forbidden,
            })
        }
    }
}

fn compare(leaf: &CompiledLeaf, text: &Text) -> bool {
    let value = text.output.as_str();
    match leaf.op {
        LeafOp::Exists => !text.missing,
        LeafOp::Eq => !text.missing && equals(&leaf.argument, value),
        LeafOp::Ne => text.missing || !equals(&leaf.argument, value),
        LeafOp::Contains => {
            !text.missing
                && matches!(&leaf.argument, LeafArgument::Text(needle) if value.contains(needle.as_str()))
        }
        LeafOp::StartsWith => {
            !text.missing
                && matches!(&leaf.argument, LeafArgument::Text(prefix) if value.starts_with(prefix.as_str()))
        }
        LeafOp::EndsWith => {
            !text.missing
                && matches!(&leaf.argument, LeafArgument::Text(suffix) if value.ends_with(suffix.as_str()))
        }
        LeafOp::Matches => {
            !text.missing
                && matches!(&leaf.argument, LeafArgument::Pattern(pattern) if pattern.is_match(value))
        }
        LeafOp::Gt => ordered(&leaf.argument, value, text.missing) == Some(Ordering::Greater),
        LeafOp::Lt => ordered(&leaf.argument, value, text.missing) == Some(Ordering::Less),
        LeafOp::In => {
            !text.missing
                && matches!(&leaf.argument, LeafArgument::List(items) if items.iter().any(|item| item == value))
        }
        LeafOp::InCidr => {
            !text.missing
                && match (&leaf.argument, value.trim().parse::<IpAddr>()) {
                    (LeafArgument::Networks(networks), Ok(address)) => {
                        networks.iter().any(|network| network.contains(&address))
                    }
                    _ => false,
                }
        }
    }
}

fn equals(argument: &LeafArgument, value: &str) -> bool {
    match argument {
        LeafArgument::Text(expected) => value == expected,
        LeafArgument::Number(expected) => value
            .trim()
            .parse::<f64>()
            .is_ok_and(|number| (number - expected).abs() < f64::EPSILON),
        _ => false,
    }
}

fn ordered(argument: &LeafArgument, value: &str, missing: bool) -> Option<Ordering> {
    let LeafArgument::Number(bound) = argument else {
        return None;
    };
    if missing {
        return None;
    }
    let number = value.trim().parse::<f64>().ok()?;
    number.partial_cmp(bound)
}
