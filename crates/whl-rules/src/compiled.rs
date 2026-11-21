use std::collections::BTreeSet;

use ipnet::IpNet;
use thiserror::Error;
use uuid::Uuid;
use whl_script::{CompiledScript, ScriptCompiler};
use whl_template::{ParseLimits, Policy, RenderTarget, SecretPolicy, StepPolicy, Template};

use crate::kind::StepKind;
use crate::model::{Action, Argument, Condition, Leaf, Op as LeafOp, Rule, Step};
use crate::pattern::Pattern;
use crate::plan::RulePlan;

#[derive(Debug, Clone)]
pub struct RuleSource {
    pub id: Uuid,
    pub rule: Rule,
}

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("{location}: the template of {field} no longer parses")]
    Template {
        location: String,
        field: &'static str,
    },
    #[error("{location}: the pattern of {field} no longer compiles")]
    Pattern {
        location: String,
        field: &'static str,
    },
    #[error("{location}: the script no longer compiles")]
    Script { location: String },
    #[error("{location}: a condition argument has the wrong shape")]
    Argument { location: String },
}

fn at(rule: usize, step: Option<usize>) -> String {
    match step {
        Some(step) => format!("rule {rule} step {step}"),
        None => format!("rule {rule} condition"),
    }
}

#[derive(Debug)]
pub(crate) enum LeafArgument {
    None,
    Text(String),
    Number(f64),
    List(Vec<String>),
    Pattern(Pattern),
    Networks(Vec<IpNet>),
}

#[derive(Debug)]
pub(crate) struct CompiledLeaf {
    pub template: Template,
    pub op: LeafOp,
    pub argument: LeafArgument,
}

#[derive(Debug)]
pub(crate) enum CompiledCondition {
    Always,
    All(Vec<CompiledCondition>),
    Any(Vec<CompiledCondition>),
    Not(Box<CompiledCondition>),
    Leaf(Box<CompiledLeaf>),
}

#[derive(Debug)]
pub(crate) struct RespondAction {
    pub status: Template,
    pub content_type: Option<Template>,
    pub headers: Vec<(String, Template)>,
    pub body: Template,
}

#[derive(Debug)]
pub(crate) enum Selector {
    JsonPath(Template),
    Pattern { regex: Pattern, group: usize },
}

#[derive(Debug)]
pub(crate) struct ExtractAction {
    pub from: Template,
    pub selector: Selector,
    pub into: String,
}

#[derive(Debug)]
pub(crate) enum TransformTarget {
    Variable(String),
    Payload,
}

#[derive(Debug)]
pub(crate) enum CompiledAction {
    Respond(Box<RespondAction>),
    SetHeader {
        name: String,
        value: Template,
    },
    RemoveHeader {
        name: String,
    },
    Delay {
        ms: u32,
    },
    Stop,
    Extract(Box<ExtractAction>),
    SetVariable {
        into: String,
        value: Template,
    },
    Transform {
        into: TransformTarget,
        template: Template,
    },
    SetStore {
        name: String,
        value: Template,
    },
    Increment {
        counter: String,
        by: i64,
    },
    Script(CompiledScript),
    Fetch,
    SideEffect,
}

#[derive(Debug)]
pub struct CompiledStep {
    pub(crate) name: Option<String>,
    pub(crate) continue_on_error: bool,
    pub(crate) kind: StepKind,
    pub(crate) action: CompiledAction,
    pub(crate) source: Step,
}

impl CompiledStep {
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn kind(&self) -> StepKind {
        self.kind
    }

    pub fn source(&self) -> &Step {
        &self.source
    }
}

#[derive(Debug)]
pub struct CompiledRule {
    pub(crate) id: Uuid,
    pub(crate) name: String,
    pub(crate) condition: CompiledCondition,
    pub(crate) steps: Vec<CompiledStep>,
    pub(crate) source: Rule,
}

impl CompiledRule {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn source(&self) -> &Rule {
        &self.source
    }

    pub fn steps(&self) -> &[CompiledStep] {
        &self.steps
    }
}

#[derive(Debug)]
pub struct CompiledRules {
    rules: Vec<CompiledRule>,
    plan: RulePlan,
    store: BTreeSet<String>,
    counters: BTreeSet<String>,
    weight: usize,
}

const TEMPLATE_WEIGHT_PER_BYTE: usize = 16;
const SCRIPT_WEIGHT_PER_BYTE: usize = 128;
const SCRIPT_WEIGHT_BASE: usize = 1024;
const RULE_WEIGHT_BASE: usize = 2048;
const SOURCE_WEIGHT_FACTOR: usize = 2;

fn condition_weight(condition: &CompiledCondition) -> usize {
    match condition {
        CompiledCondition::Always => 0,
        CompiledCondition::All(items) | CompiledCondition::Any(items) => {
            items.iter().map(condition_weight).sum()
        }
        CompiledCondition::Not(inner) => condition_weight(inner),
        CompiledCondition::Leaf(leaf) => {
            let argument = match &leaf.argument {
                LeafArgument::Pattern(pattern) => pattern.weight_bytes(),
                _ => 0,
            };
            leaf.template.source().len() * TEMPLATE_WEIGHT_PER_BYTE + argument
        }
    }
}

fn template_weight(template: &Template) -> usize {
    template.source().len() * TEMPLATE_WEIGHT_PER_BYTE
}

fn action_weight(action: &CompiledAction) -> usize {
    match action {
        CompiledAction::Respond(respond) => {
            template_weight(&respond.status)
                + respond.content_type.as_ref().map_or(0, template_weight)
                + respond
                    .headers
                    .iter()
                    .map(|(_, template)| template_weight(template))
                    .sum::<usize>()
                + template_weight(&respond.body)
        }
        CompiledAction::SetHeader { value, .. }
        | CompiledAction::SetVariable { value, .. }
        | CompiledAction::SetStore { value, .. } => template_weight(value),
        CompiledAction::Transform { template, .. } => template_weight(template),
        CompiledAction::Extract(extract) => {
            template_weight(&extract.from)
                + match &extract.selector {
                    Selector::JsonPath(template) => template_weight(template),
                    Selector::Pattern { regex, .. } => regex.weight_bytes(),
                }
        }
        CompiledAction::Script(script) => {
            script.source_bytes() * SCRIPT_WEIGHT_PER_BYTE + SCRIPT_WEIGHT_BASE
        }
        CompiledAction::RemoveHeader { .. }
        | CompiledAction::Delay { .. }
        | CompiledAction::Stop
        | CompiledAction::Increment { .. }
        | CompiledAction::Fetch
        | CompiledAction::SideEffect => 0,
    }
}

fn rule_weight(rule: &CompiledRule) -> usize {
    let source = serde_json::to_vec(&rule.source).map_or(0, |bytes| bytes.len());
    RULE_WEIGHT_BASE
        + source * SOURCE_WEIGHT_FACTOR
        + condition_weight(&rule.condition)
        + rule
            .steps
            .iter()
            .map(|step| action_weight(&step.action))
            .sum::<usize>()
}

struct Collector<'a> {
    names: &'a BTreeSet<String>,
    store: BTreeSet<String>,
    counters: BTreeSet<String>,
}

impl Collector<'_> {
    fn parse(&mut self, source: &str, target: RenderTarget) -> Option<Template> {
        let policy = Policy {
            target,
            secrets: SecretPolicy::Forbidden,
            steps: StepPolicy::Only(self.names.clone()),
            limits: ParseLimits::default(),
        };
        let template = Template::parse(source, &policy).ok()?;
        self.store.extend(template.referenced_store());
        self.counters.extend(template.referenced_counters());
        Some(template)
    }
}

impl CompiledRules {
    pub fn compile(
        sources: Vec<RuleSource>,
        scripts: &ScriptCompiler,
    ) -> Result<Self, CompileError> {
        let enabled: Vec<RuleSource> = sources
            .into_iter()
            .filter(|source| source.rule.enabled)
            .collect();
        let names: BTreeSet<String> = enabled
            .iter()
            .flat_map(|source| source.rule.steps.iter())
            .filter_map(|step| step.name.clone())
            .collect();
        let mut collector = Collector {
            names: &names,
            store: BTreeSet::new(),
            counters: BTreeSet::new(),
        };
        let mut rules = Vec::with_capacity(enabled.len());
        for (index, source) in enabled.into_iter().enumerate() {
            rules.push(compile_rule(index, source, scripts, &mut collector)?);
        }
        let plan = RulePlan::of(
            &rules
                .iter()
                .map(|rule| rule.source.clone())
                .collect::<Vec<_>>(),
        );
        let weight = rules.iter().map(rule_weight).sum();
        Ok(Self {
            rules,
            plan,
            store: collector.store,
            counters: collector.counters,
            weight,
        })
    }

    pub fn rules(&self) -> &[CompiledRule] {
        &self.rules
    }

    pub fn plan(&self) -> RulePlan {
        self.plan
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn weight_bytes(&self) -> usize {
        self.weight
    }

    pub fn store_names(&self) -> &BTreeSet<String> {
        &self.store
    }

    pub fn counter_names(&self) -> &BTreeSet<String> {
        &self.counters
    }

    pub fn variable_names(&self) -> Vec<String> {
        self.store.union(&self.counters).cloned().collect()
    }
}

fn compile_rule(
    index: usize,
    source: RuleSource,
    scripts: &ScriptCompiler,
    collector: &mut Collector<'_>,
) -> Result<CompiledRule, CompileError> {
    let condition = compile_condition(index, &source.rule.condition, collector)?;
    let mut steps = Vec::with_capacity(source.rule.steps.len());
    for (step_index, step) in source.rule.steps.iter().enumerate() {
        steps.push(compile_step(index, step_index, step, scripts, collector)?);
    }
    Ok(CompiledRule {
        id: source.id,
        name: source.rule.name.clone(),
        condition,
        steps,
        source: source.rule,
    })
}

fn compile_condition(
    rule: usize,
    condition: &Condition,
    collector: &mut Collector<'_>,
) -> Result<CompiledCondition, CompileError> {
    Ok(match condition {
        Condition::Always => CompiledCondition::Always,
        Condition::All(items) => CompiledCondition::All(
            items
                .iter()
                .map(|item| compile_condition(rule, item, collector))
                .collect::<Result<_, _>>()?,
        ),
        Condition::Any(items) => CompiledCondition::Any(
            items
                .iter()
                .map(|item| compile_condition(rule, item, collector))
                .collect::<Result<_, _>>()?,
        ),
        Condition::Not(inner) => {
            CompiledCondition::Not(Box::new(compile_condition(rule, inner, collector)?))
        }
        Condition::Leaf(leaf) => {
            CompiledCondition::Leaf(Box::new(compile_leaf(rule, leaf, collector)?))
        }
    })
}

fn compile_leaf(
    rule: usize,
    leaf: &Leaf,
    collector: &mut Collector<'_>,
) -> Result<CompiledLeaf, CompileError> {
    let template = collector
        .parse(&format!("{{{{ {} }}}}", leaf.path), RenderTarget::Body)
        .ok_or_else(|| CompileError::Template {
            location: at(rule, None),
            field: "condition",
        })?;
    let argument = match (leaf.op, &leaf.argument) {
        (_, None) => LeafArgument::None,
        (LeafOp::Matches, Some(Argument::Text(pattern))) => LeafArgument::Pattern(
            Pattern::compile(pattern).map_err(|_| CompileError::Pattern {
                location: at(rule, None),
                field: "condition",
            })?,
        ),
        (LeafOp::InCidr, Some(Argument::List(items))) => LeafArgument::Networks(
            items
                .iter()
                .map(|item| item.parse::<IpNet>())
                .collect::<Result<_, _>>()
                .map_err(|_| CompileError::Argument {
                    location: at(rule, None),
                })?,
        ),
        (_, Some(Argument::List(items))) => LeafArgument::List(items.clone()),
        (_, Some(Argument::Text(text))) => LeafArgument::Text(text.clone()),
        (_, Some(Argument::Number(number))) => {
            LeafArgument::Number(number.as_f64().ok_or_else(|| CompileError::Argument {
                location: at(rule, None),
            })?)
        }
    };
    Ok(CompiledLeaf {
        template,
        op: leaf.op,
        argument,
    })
}

fn template_of(
    collector: &mut Collector<'_>,
    rule: usize,
    step: usize,
    field: &'static str,
    source: &str,
    target: RenderTarget,
) -> Result<Template, CompileError> {
    collector
        .parse(source, target)
        .ok_or_else(|| CompileError::Template {
            location: at(rule, Some(step)),
            field,
        })
}

fn compile_step(
    rule: usize,
    index: usize,
    step: &Step,
    scripts: &ScriptCompiler,
    collector: &mut Collector<'_>,
) -> Result<CompiledStep, CompileError> {
    let kind = StepKind::of(&step.action);
    let mut template = |field: &'static str, source: &str, target: RenderTarget| {
        template_of(collector, rule, index, field, source, target)
    };
    let action = match &step.action {
        Action::Respond {
            status,
            content_type,
            headers,
            body,
        } => {
            let status = template("status", status, RenderTarget::Status)?;
            let content_type = content_type
                .as_deref()
                .map(|source| template("content_type", source, RenderTarget::HeaderValue))
                .transpose()?;
            let mut compiled = Vec::with_capacity(headers.len());
            for header in headers {
                compiled.push((
                    header.name.clone(),
                    template("headers", &header.value, RenderTarget::HeaderValue)?,
                ));
            }
            let body = template("body", body, RenderTarget::Body)?;
            CompiledAction::Respond(Box::new(RespondAction {
                status,
                content_type,
                headers: compiled,
                body,
            }))
        }
        Action::SetHeader { name, value } => CompiledAction::SetHeader {
            name: name.clone(),
            value: template("value", value, RenderTarget::HeaderValue)?,
        },
        Action::RemoveHeader { name } => CompiledAction::RemoveHeader { name: name.clone() },
        Action::Delay { ms } => CompiledAction::Delay { ms: *ms },
        Action::Stop => CompiledAction::Stop,
        Action::Extract {
            from,
            json_path,
            regex,
            group,
            into,
        } => {
            let from = template("from", &format!("{{{{ {from} }}}}"), RenderTarget::Body)?;
            let selector = match (json_path, regex) {
                (Some(path), _) => {
                    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
                    Selector::JsonPath(template(
                        "json_path",
                        &format!("{{{{ json(\"{escaped}\") }}}}"),
                        RenderTarget::Body,
                    )?)
                }
                (None, Some(pattern)) => Selector::Pattern {
                    regex: Pattern::compile(pattern).map_err(|_| CompileError::Pattern {
                        location: at(rule, Some(index)),
                        field: "regex",
                    })?,
                    group: usize::try_from(group.unwrap_or(1)).unwrap_or(1),
                },
                (None, None) => {
                    return Err(CompileError::Pattern {
                        location: at(rule, Some(index)),
                        field: "selector",
                    });
                }
            };
            CompiledAction::Extract(Box::new(ExtractAction {
                from,
                selector,
                into: into.clone(),
            }))
        }
        Action::SetVar { into, value } => CompiledAction::SetVariable {
            into: into.clone(),
            value: template("value", value, RenderTarget::Body)?,
        },
        Action::Transform {
            into,
            template: source,
        } => CompiledAction::Transform {
            into: if into == "payload" {
                TransformTarget::Payload
            } else {
                TransformTarget::Variable(into.clone())
            },
            template: template("template", source, RenderTarget::Body)?,
        },
        Action::SetStore { name, value } => CompiledAction::SetStore {
            name: name.clone(),
            value: template("value", value, RenderTarget::Body)?,
        },
        Action::Increment { counter, by } => CompiledAction::Increment {
            counter: counter.clone(),
            by: *by,
        },
        Action::Script { source } => {
            CompiledAction::Script(scripts.compile(source).map_err(|_| CompileError::Script {
                location: at(rule, Some(index)),
            })?)
        }
        Action::Http { .. } => CompiledAction::Fetch,
        Action::Forward { .. } | Action::Notify { .. } | Action::StoreS3 { .. } => {
            CompiledAction::SideEffect
        }
    };
    Ok(CompiledStep {
        name: step.name.clone(),
        continue_on_error: step.continue_on_error,
        kind,
        action,
        source: step.clone(),
    })
}
