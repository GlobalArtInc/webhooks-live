use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;
use whl_core::content_type::ContentType;
use whl_core::header::HeaderValue;
use whl_core::status::HttpStatus;
use whl_script::{CompiledScript, ScriptError, ScriptOutput, ScriptPhase, ScriptResponse};
use whl_telemetry::series::RULE_STEPS_TOTAL;
use whl_template::{
    BodyData, ChaChaRandom, Context, NameValue, RandomSeed, ResponseBudget, StepData, SystemClock,
    Template,
};

use crate::after::{AfterPart, DeferredStep, Resume, Snapshot};
use crate::compiled::{
    CompiledAction, CompiledRule, CompiledRules, CompiledStep, ExtractAction, HttpAction,
    RespondAction, Selector, TransformTarget,
};
use crate::deferred::side_effect_call;
use crate::draft::{HeaderOp, ResponseDraft};
use crate::eval::{Evaluator, RenderIssue, Text, charge_pattern, render};
use crate::failure::{Failure, FailureCode};
use crate::kind::StepKind;
use crate::log::{
    Entry, MAX_DETAIL_BYTES, Outcome, RuleEntry, StepEntry, bound_output, bound_text,
};
use crate::model::{BodySource, S3Content};

pub const RESPOND_BUDGET: Duration = Duration::from_millis(100);
pub const STEP_BUDGET: Duration = Duration::from_secs(2);
pub const MAX_VARS_BYTES: usize = 16 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024;
pub const MAX_HTTP_URL_BYTES: usize = 2048;
pub const MAX_HTTP_BODY_BYTES: usize = 1024 * 1024;
const MAX_LOGGED_VALUE_BYTES: usize = 512;
const MAX_LOGGED_SCRIPT_BYTES: usize = 1024;
const DEFAULT_SCRIPT_STATUS: u16 = 200;
const MAX_INTEGER_DIGITS: usize = 18;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Live,
    DryRun,
    After,
}

pub struct MachineInput {
    pub rules: Arc<CompiledRules>,
    pub context: Context,
    pub seed: RandomSeed,
    pub budget: Duration,
    pub mode: Mode,
    pub tenant: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StoreCall {
    Set { name: String, value: Value },
    Add { name: String, by: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreValue {
    Written,
    Counter(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    Unavailable,
    Timeout,
    EndpointGone,
    Rejected,
    LimitReached,
    NotACounter,
}

pub struct ScriptCall {
    pub phase: ScriptPhase,
    pub script: CompiledScript,
    pub request: Value,
    pub vars: Map<String, Value>,
    pub now: OffsetDateTime,
    pub seed: whl_script::RandomSeed,
    pub remaining: Duration,
    pub tenant: String,
}

#[derive(Clone, PartialEq)]
pub struct HttpCall {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_ms: u32,
}

impl fmt::Debug for HttpCall {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpCall")
            .field("method", &self.method)
            .field("url_bytes", &self.url.len())
            .field("headers", &self.headers.len())
            .field("body_bytes", &self.body.as_ref().map(String::len))
            .field("timeout_ms", &self.timeout_ms)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardCall {
    pub forward_rule_id: Option<Uuid>,
    pub target_url: Option<String>,
    pub method: Option<String>,
    pub content_type: Option<String>,
    pub header_allowlist: Vec<String>,
    pub body_from: BodySource,
    pub payload: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyCall {
    pub channel_id: Uuid,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreS3Call {
    pub bucket_url: String,
    pub prefix: String,
    pub region: String,
    pub access_key_secret: String,
    pub secret_key_secret: String,
    pub content: S3Content,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutboundCall {
    Http(HttpCall),
    Forward(ForwardCall),
    Notify(NotifyCall),
    StoreS3(StoreS3Call),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutboundStep {
    pub rule_id: Uuid,
    pub rule_name: String,
    pub step: usize,
    pub position: usize,
    pub name: Option<String>,
    pub kind: StepKind,
    pub continue_on_error: bool,
    pub call: OutboundCall,
}

#[derive(Debug, Clone)]
pub struct OutboundDone {
    pub outcome: Outcome,
    pub detail: Option<String>,
    pub output: Value,
    pub data: Option<StepData>,
}

pub enum Effect {
    Script(Box<ScriptCall>),
    Store(StoreCall),
    Outbound(Box<OutboundStep>),
}

pub enum Answer {
    Script(Box<Result<ScriptOutput, ScriptError>>),
    Store(Result<StoreValue, StateError>),
    Outbound(Box<Result<OutboundDone, Failure>>),
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub position: usize,
    pub vars: Map<String, Value>,
    pub steps: BTreeMap<String, StepData>,
    pub payload: Option<String>,
    pub log: Vec<Entry>,
}

#[derive(Debug, Clone, Default)]
pub struct Resumption {
    pub position: usize,
    pub payload: Option<String>,
    pub steps: BTreeMap<String, StepData>,
    pub log: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunOutcome {
    Succeeded,
    Failed(Failure),
}

#[derive(Debug)]
pub struct Finished {
    pub outcome: RunOutcome,
    pub response: ResponseDraft,
    pub log: Vec<Entry>,
    pub vars: Map<String, Value>,
    pub steps: BTreeMap<String, StepData>,
    pub after: AfterPart,
    pub stopped: bool,
}

impl Finished {
    pub fn failed(failure: Failure) -> Self {
        Self {
            outcome: RunOutcome::Failed(failure),
            response: ResponseDraft::default(),
            log: Vec::new(),
            vars: Map::new(),
            steps: BTreeMap::new(),
            after: AfterPart::default(),
            stopped: false,
        }
    }

    pub fn failure(&self) -> Option<&Failure> {
        match &self.outcome {
            RunOutcome::Failed(failure) => Some(failure),
            RunOutcome::Succeeded => None,
        }
    }
}

pub enum Advance {
    Effect(Effect),
    Checkpoint,
    Finished(Box<Finished>),
}

struct Pending {
    started: Instant,
    written: Option<(String, Value)>,
}

struct Done {
    outcome: Outcome,
    detail: Option<String>,
    output: Option<Value>,
}

impl Done {
    fn ok(output: Value) -> Self {
        Self {
            outcome: Outcome::Ok,
            detail: None,
            output: Some(output),
        }
    }

    fn plain(outcome: Outcome, detail: &str) -> Self {
        Self {
            outcome,
            detail: Some(detail.to_owned()),
            output: None,
        }
    }
}

enum Produced {
    Done(Result<Done, Failure>),
    Effect(Effect, Option<(String, Value)>),
}

pub struct Machine {
    rules: Arc<CompiledRules>,
    context: Context,
    random: ChaChaRandom,
    seed: [u8; 32],
    clock: SystemClock,
    budget: ResponseBudget,
    mode: Mode,
    tenant: String,
    rule: usize,
    step: usize,
    entered: bool,
    response: ResponseDraft,
    payload: Option<String>,
    snapshots: Vec<Snapshot>,
    deferred: Vec<DeferredStep>,
    log: Vec<Entry>,
    stopped: bool,
    failure: Option<Failure>,
    pending: Option<Pending>,
    checkpoint_due: bool,
    step_data: Option<StepData>,
}

fn locate(rules: &CompiledRules, position: usize) -> (usize, usize) {
    let mut remaining = position;
    for (index, rule) in rules.rules().iter().enumerate() {
        let length = rule.steps.len();
        if remaining < length {
            return (index, remaining);
        }
        remaining -= length;
    }
    (rules.rules().len(), 0)
}

impl Machine {
    pub fn new(input: MachineInput) -> Self {
        let clock = SystemClock::new();
        let budget = ResponseBudget::start(&clock, input.budget);
        Self {
            rules: input.rules,
            context: input.context,
            random: ChaChaRandom::from_seed(&input.seed),
            seed: *input.seed.unpack(),
            clock,
            budget,
            mode: input.mode,
            tenant: input.tenant,
            rule: 0,
            step: 0,
            entered: false,
            response: ResponseDraft::default(),
            payload: None,
            snapshots: Vec::new(),
            deferred: Vec::new(),
            log: Vec::new(),
            stopped: false,
            failure: None,
            pending: None,
            checkpoint_due: false,
            step_data: None,
        }
    }

    pub fn resumed(input: MachineInput, resumption: Resumption) -> Self {
        let mut machine = Self::new(input);
        machine.mode = Mode::After;
        let (rule, step) = locate(&machine.rules, resumption.position);
        machine.rule = rule;
        machine.step = step;
        machine.payload = resumption.payload;
        machine.log = resumption.log;
        machine.context.steps = resumption.steps;
        machine.entered = step > 0 || machine.was_entered(rule);
        machine
    }

    fn was_entered(&self, rule: usize) -> bool {
        let Some(candidate) = self.rules.rules().get(rule) else {
            return false;
        };
        matches!(
            self.log.last(),
            Some(Entry::Rule(entry)) if entry.matched && entry.rule_id == candidate.id
        )
    }

    pub fn position(&self) -> usize {
        let before: usize = self
            .rules
            .rules()
            .iter()
            .take(self.rule)
            .map(|rule| rule.steps.len())
            .sum();
        before + self.step
    }

    pub fn progress(&self) -> Progress {
        Progress {
            position: self.position(),
            vars: self.vars_map(),
            steps: self.context.steps.clone(),
            payload: self.payload.clone(),
            log: self.log.clone(),
        }
    }

    pub fn advance(&mut self) -> Advance {
        if self.pending.take().is_some() {
            self.failure = Some(Failure::new(
                FailureCode::Internal,
                "an effect was left unanswered",
            ));
        }
        let rules = Arc::clone(&self.rules);
        let plan = rules.plan();
        let after = self.mode == Mode::After;
        loop {
            if self.failure.is_some() || self.stopped {
                return self.finish(&rules);
            }
            let Some(rule) = rules.rules().get(self.rule) else {
                return self.finish(&rules);
            };
            if !after && plan.cut().is_none_or(|cut| self.rule > cut.rule) {
                return self.finish(&rules);
            }
            if self.checkpoint_due {
                self.checkpoint_due = false;
                return Advance::Checkpoint;
            }
            if after {
                self.budget = ResponseBudget::start(&self.clock, STEP_BUDGET);
            }
            if !self.entered {
                if !after && self.budget_spent() {
                    self.fail_budget();
                    continue;
                }
                match self.enter(rule) {
                    Ok(true) => {
                        self.entered = true;
                        self.step = 0;
                    }
                    Ok(false) => {
                        self.rule += 1;
                        self.step = 0;
                        self.checkpoint_due = after;
                        continue;
                    }
                    Err(failure) => {
                        self.failure = Some(failure);
                        continue;
                    }
                }
            }
            let Some(step) = rule.steps.get(self.step) else {
                self.rule += 1;
                self.step = 0;
                self.entered = false;
                continue;
            };
            if !after && plan.is_after(self.rule, self.step) {
                return self.finish(&rules);
            }
            if !after && self.budget_spent() {
                self.fail_budget();
                continue;
            }
            if let Some(effect) = self.execute(rule, step) {
                return Advance::Effect(effect);
            }
        }
    }

    pub fn resume(&mut self, answer: Answer) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let rules = Arc::clone(&self.rules);
        let Some((rule, step)) = rules
            .rules()
            .get(self.rule)
            .and_then(|rule| rule.steps.get(self.step).map(|step| (rule, step)))
        else {
            self.failure = Some(Failure::new(
                FailureCode::Internal,
                "the answered step is missing",
            ));
            return;
        };
        let result = match answer {
            Answer::Script(result) => self.absorb_script(*result),
            Answer::Store(result) => self.absorb_store(result, pending.written),
            Answer::Outbound(result) => self.absorb_outbound(*result),
        };
        let index = self.step;
        self.conclude(rule, index, step, pending.started.elapsed(), result);
        self.step += 1;
    }

    pub fn is_dry_run(&self) -> bool {
        self.mode == Mode::DryRun
    }

    fn budget_spent(&self) -> bool {
        self.budget.remaining(&self.clock).is_zero()
    }

    fn fail_budget(&mut self) {
        self.failure = Some(Failure::new(
            FailureCode::Budget,
            "the time budget for rules before the response is spent",
        ));
    }

    fn vars_map(&self) -> Map<String, Value> {
        self.context
            .vars
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    fn finish(&mut self, rules: &Arc<CompiledRules>) -> Advance {
        let outcome = match self.failure.take() {
            Some(failure) => RunOutcome::Failed(failure),
            None => RunOutcome::Succeeded,
        };
        let after = match outcome {
            RunOutcome::Failed(_) => AfterPart::default(),
            RunOutcome::Succeeded if self.mode == Mode::After => AfterPart::default(),
            RunOutcome::Succeeded => {
                let resume = (!self.stopped && self.rule < rules.rules().len()).then_some(Resume {
                    rule: self.rule,
                    step: if self.entered { self.step } else { 0 },
                });
                AfterPart {
                    snapshots: std::mem::take(&mut self.snapshots),
                    deferred: std::mem::take(&mut self.deferred),
                    resume,
                    vars: self.vars_map(),
                    payload: self.payload.clone(),
                }
            }
        };
        Advance::Finished(Box::new(Finished {
            outcome,
            response: std::mem::take(&mut self.response),
            log: std::mem::take(&mut self.log),
            vars: self.vars_map(),
            steps: self.context.steps.clone(),
            after,
            stopped: self.stopped,
        }))
    }

    fn enter(&mut self, rule: &CompiledRule) -> Result<bool, Failure> {
        let mut evaluator = Evaluator {
            context: &self.context,
            random: &mut self.random,
            clock: &self.clock,
            budget: &self.budget,
        };
        let matched = evaluator.condition(&rule.condition);
        self.log.push(Entry::Rule(RuleEntry {
            rule_id: rule.id,
            rule_name: rule.name.clone(),
            matched: matched.as_ref().is_ok_and(|matched| *matched),
        }));
        matched.map_err(|issue| issue.failure)
    }

    fn execute(&mut self, rule: &CompiledRule, step: &CompiledStep) -> Option<Effect> {
        let started = Instant::now();
        let index = self.step;
        let produced = self.produce(rule, index, step);
        match produced {
            Produced::Done(result) => {
                self.conclude(rule, index, step, started.elapsed(), result);
                self.step += 1;
                None
            }
            Produced::Effect(effect, written) => {
                self.pending = Some(Pending { started, written });
                Some(effect)
            }
        }
    }

    fn produce(&mut self, rule: &CompiledRule, index: usize, step: &CompiledStep) -> Produced {
        let after = self.mode == Mode::After;
        match &step.action {
            CompiledAction::Respond(_)
            | CompiledAction::SetHeader { .. }
            | CompiledAction::RemoveHeader { .. }
            | CompiledAction::Delay { .. }
                if after =>
            {
                Produced::Done(Ok(Done::plain(
                    Outcome::NoOp,
                    "the response was already sent",
                )))
            }
            CompiledAction::Respond(action) => Produced::Done(self.respond(action)),
            CompiledAction::SetHeader { name, value } => {
                Produced::Done(self.set_header(name, value))
            }
            CompiledAction::RemoveHeader { name } => {
                self.response
                    .header_ops
                    .push(HeaderOp::Remove(name.to_ascii_lowercase()));
                Produced::Done(Ok(Done::ok(json!({"name": name}))))
            }
            CompiledAction::Delay { ms } => {
                self.response.add_delay(*ms);
                Produced::Done(Ok(Done::ok(json!({"ms": ms}))))
            }
            CompiledAction::Stop => {
                self.stopped = true;
                Produced::Done(Ok(Done::plain(
                    Outcome::Ok,
                    "later steps and rules are skipped",
                )))
            }
            CompiledAction::Extract(action) => Produced::Done(self.extract(action)),
            CompiledAction::SetVariable { into, value } => Produced::Done(
                self.render_value(value)
                    .map_err(|issue| issue.failure)
                    .and_then(|text| {
                        let output = json!({"into": into, "value": logged(&text.output)});
                        self.set_variable(into, Value::String(text.output))
                            .map(|()| Done::ok(output))
                    }),
            ),
            CompiledAction::Transform { into, template } => {
                Produced::Done(self.transform(into, template))
            }
            CompiledAction::SetStore { name, value } => self.set_store(name, value),
            CompiledAction::Increment { counter, by } => self.increment(counter, *by),
            CompiledAction::Script(script) => match self.script_call(script, index) {
                Ok(call) => Produced::Effect(Effect::Script(Box::new(call)), None),
                Err(failure) => Produced::Done(Err(failure)),
            },
            CompiledAction::Fetch(action) if after => self.http_effect(rule, index, step, action),
            CompiledAction::Fetch(_) => Produced::Done(Err(Failure::new(
                FailureCode::Http,
                "HTTP steps are not available before the response",
            ))),
            CompiledAction::SideEffect { message } if after => {
                self.side_effect(rule, index, step, message.as_ref())
            }
            CompiledAction::SideEffect { .. } => Produced::Done(self.defer(rule, index, step)),
        }
    }

    fn outbound(
        &self,
        rule: &CompiledRule,
        index: usize,
        step: &CompiledStep,
        call: OutboundCall,
    ) -> Produced {
        Produced::Effect(
            Effect::Outbound(Box::new(OutboundStep {
                rule_id: rule.id,
                rule_name: rule.name.clone(),
                step: index,
                position: self.position(),
                name: step.name.clone(),
                kind: step.kind,
                continue_on_error: step.continue_on_error,
                call,
            })),
            None,
        )
    }

    fn render_strict(&mut self, template: &Template) -> Result<String, Failure> {
        let text = self.render_value(template).map_err(|issue| issue.failure)?;
        if text.missing {
            return Err(Failure::new(
                FailureCode::Template,
                format!(
                    "{} is not defined",
                    text.first_missing.as_deref().unwrap_or("a variable")
                ),
            ));
        }
        Ok(text.output)
    }

    fn render_http(&mut self, action: &HttpAction) -> Result<HttpCall, Failure> {
        let url = self.render_strict(&action.url)?;
        if url.len() > MAX_HTTP_URL_BYTES {
            return Err(Failure::new(
                FailureCode::Template,
                format!("the URL is longer than {MAX_HTTP_URL_BYTES} bytes"),
            ));
        }
        let mut headers = Vec::with_capacity(action.headers.len());
        for (name, template) in &action.headers {
            headers.push((name.clone(), self.render_strict(template)?));
        }
        let body = match &action.body {
            Some(template) => Some(self.render_strict(template)?),
            None => None,
        };
        if body
            .as_ref()
            .is_some_and(|text| text.len() > MAX_HTTP_BODY_BYTES)
        {
            return Err(Failure::new(
                FailureCode::Template,
                format!("the body is longer than {MAX_HTTP_BODY_BYTES} bytes"),
            ));
        }
        Ok(HttpCall {
            method: action.method.clone(),
            url,
            headers,
            body,
            timeout_ms: action.timeout_ms,
        })
    }

    fn http_effect(
        &mut self,
        rule: &CompiledRule,
        index: usize,
        step: &CompiledStep,
        action: &HttpAction,
    ) -> Produced {
        match self.render_http(action) {
            Ok(call) => self.outbound(rule, index, step, OutboundCall::Http(call)),
            Err(failure) => Produced::Done(Err(failure)),
        }
    }

    fn side_effect(
        &mut self,
        rule: &CompiledRule,
        index: usize,
        step: &CompiledStep,
        message: Option<&Template>,
    ) -> Produced {
        let text = match message {
            Some(template) => match self.render_value(template) {
                Ok(text) => Some(text.output),
                Err(issue) => return Produced::Done(Err(issue.failure)),
            },
            None => None,
        };
        match side_effect_call(&step.source.action, text, self.payload.as_deref()) {
            Ok(call) => self.outbound(rule, index, step, call),
            Err(failure) => Produced::Done(Err(failure)),
        }
    }

    fn render_value(&mut self, template: &Template) -> Result<Text, RenderIssue> {
        render(
            &self.context,
            &mut self.random,
            &self.clock,
            template,
            &self.budget,
        )
    }

    fn set_variable(&mut self, name: &str, value: Value) -> Result<(), Failure> {
        let previous = self.context.vars.insert(name.to_owned(), value);
        let size = serde_json::to_vec(&self.context.vars).map_or(usize::MAX, |bytes| bytes.len());
        if size > MAX_VARS_BYTES {
            match previous {
                Some(value) => {
                    self.context.vars.insert(name.to_owned(), value);
                }
                None => {
                    self.context.vars.remove(name);
                }
            }
            return Err(Failure::new(
                FailureCode::Template,
                format!("the run variables would exceed {MAX_VARS_BYTES} bytes"),
            ));
        }
        Ok(())
    }

    fn respond(&mut self, action: &RespondAction) -> Result<Done, Failure> {
        let allowance = self.budget.remaining(&self.clock).min(RESPOND_BUDGET);
        if allowance.is_zero() {
            return Err(Failure::new(
                FailureCode::Budget,
                "no time is left to render the response",
            ));
        }
        let local = ResponseBudget::start(&self.clock, allowance);
        let mut evaluator = Evaluator {
            context: &self.context,
            random: &mut self.random,
            clock: &self.clock,
            budget: &local,
        };
        let status_text = evaluator
            .render(&action.status)
            .map_err(|issue| issue.failure)?;
        let status = parse_status(&status_text.output).ok_or_else(|| {
            Failure::new(
                FailureCode::Template,
                "the status does not render to a code",
            )
        })?;
        let body = evaluator
            .render(&action.body)
            .map_err(|issue| issue.failure)?;
        let mut headers = Vec::with_capacity(action.headers.len());
        let mut dropped = 0usize;
        for (name, template) in &action.headers {
            match evaluator.render(template) {
                Ok(text) => {
                    if HeaderValue::parse(&text.output).is_ok() {
                        headers.push((name.clone(), text.output));
                    } else {
                        dropped += 1;
                    }
                }
                Err(issue) if issue.only_forbidden => dropped += 1,
                Err(issue) => return Err(issue.failure),
            }
        }
        let content_type = match &action.content_type {
            None => None,
            Some(template) => {
                let text = evaluator.render(template).map_err(|issue| issue.failure)?;
                ContentType::parse(&text.output).map_err(|_| {
                    Failure::new(
                        FailureCode::Template,
                        "the content type does not render to a media type",
                    )
                })?;
                Some(text.output)
            }
        };
        let output = json!({
            "status": status.unpack(),
            "content_type": content_type,
            "headers": headers.len(),
            "body_bytes": body.output.len(),
        });
        self.response.replace(
            status.unpack(),
            content_type,
            headers,
            body.output.into_bytes(),
        );
        Ok(Done {
            outcome: Outcome::Ok,
            detail: (dropped > 0).then(|| format!("{dropped} header(s) dropped as invalid")),
            output: Some(output),
        })
    }

    fn set_header(&mut self, name: &str, value: &Template) -> Result<Done, Failure> {
        let rendered = match self.render_value(value) {
            Ok(text) => Some(text.output),
            Err(issue) if issue.only_forbidden => None,
            Err(issue) => return Err(issue.failure),
        };
        match rendered.filter(|text| HeaderValue::parse(text).is_ok()) {
            Some(text) => {
                self.response
                    .header_ops
                    .push(HeaderOp::Set(name.to_owned(), text));
                Ok(Done::ok(json!({"name": name})))
            }
            None => Ok(Done::plain(
                Outcome::Ok,
                "the header was dropped as invalid",
            )),
        }
    }

    fn extract(&mut self, action: &ExtractAction) -> Result<Done, Failure> {
        let source = self
            .render_value(&action.from)
            .map_err(|issue| issue.failure)?;
        let found = match &action.selector {
            Selector::JsonPath(template) => {
                let mut scratch = Context::new(self.context.now);
                scratch.request.body = BodyData::from_text(&source.output);
                let text = render(
                    &scratch,
                    &mut self.random,
                    &self.clock,
                    template,
                    &self.budget,
                )
                .map_err(|issue| issue.failure)?;
                (!text.missing).then_some(text.output)
            }
            Selector::Pattern { regex, group } => {
                charge_pattern(
                    regex.capture_cost(source.output.len()),
                    &self.budget,
                    &self.clock,
                )
                .map_err(|issue| issue.failure)?;
                regex
                    .captures(&source.output)
                    .and_then(|captures| captures.get(*group))
                    .map(|found| found.as_str().to_owned())
            }
        };
        match found {
            Some(value) => {
                let output = json!({"into": action.into, "value": logged(&value)});
                self.set_variable(&action.into, Value::String(value))?;
                Ok(Done::ok(output))
            }
            None => Ok(Done::plain(Outcome::Ok, "no match")),
        }
    }

    fn transform(&mut self, into: &TransformTarget, template: &Template) -> Result<Done, Failure> {
        let text = self.render_value(template).map_err(|issue| issue.failure)?;
        match into {
            TransformTarget::Variable(name) => {
                let output = json!({"into": name, "value": logged(&text.output)});
                self.set_variable(name, Value::String(text.output))?;
                Ok(Done::ok(output))
            }
            TransformTarget::Payload => {
                if text.output.len() > MAX_PAYLOAD_BYTES {
                    return Err(Failure::new(
                        FailureCode::Template,
                        format!("the payload would exceed {MAX_PAYLOAD_BYTES} bytes"),
                    ));
                }
                let output = json!({"into": "payload", "bytes": text.output.len()});
                self.payload = Some(text.output);
                Ok(Done::ok(output))
            }
        }
    }

    fn set_store(&mut self, name: &str, template: &Template) -> Produced {
        let text = match self.render_value(template) {
            Ok(text) => text,
            Err(issue) => return Produced::Done(Err(issue.failure)),
        };
        let value = store_value(&text.output);
        match self.mode {
            Mode::Live | Mode::After => Produced::Effect(
                Effect::Store(StoreCall::Set {
                    name: name.to_owned(),
                    value: value.clone(),
                }),
                Some((name.to_owned(), value)),
            ),
            Mode::DryRun => {
                let output = json!({"name": name, "value": logged(&text.output)});
                self.remember_store(name, value);
                Produced::Done(Ok(Done {
                    outcome: Outcome::NoOp,
                    detail: Some(format!("would set store.{name}")),
                    output: Some(output),
                }))
            }
        }
    }

    fn increment(&mut self, counter: &str, by: i64) -> Produced {
        match self.mode {
            Mode::Live | Mode::After => Produced::Effect(
                Effect::Store(StoreCall::Add {
                    name: counter.to_owned(),
                    by,
                }),
                None,
            ),
            Mode::DryRun => {
                let current = self.context.counters.get(counter).copied().unwrap_or(0);
                let next = current.saturating_add(by);
                self.remember_store(counter, Value::from(next));
                Produced::Done(Ok(Done {
                    outcome: Outcome::NoOp,
                    detail: Some(format!("would add {by} to counter.{counter}")),
                    output: Some(json!({"counter": counter, "value": next})),
                }))
            }
        }
    }

    fn remember_store(&mut self, name: &str, value: Value) {
        if let Some(number) = value.as_i64() {
            self.context.counters.insert(name.to_owned(), number);
        } else {
            self.context.counters.remove(name);
        }
        self.context.store.insert(name.to_owned(), value);
    }

    fn script_call(
        &mut self,
        script: &CompiledScript,
        index: usize,
    ) -> Result<ScriptCall, Failure> {
        let remaining = self.budget.remaining(&self.clock);
        if remaining.is_zero() {
            return Err(Failure::new(
                FailureCode::Budget,
                "no time is left to run the script",
            ));
        }
        let mut material = Vec::with_capacity(48);
        material.extend_from_slice(&self.seed);
        material.extend_from_slice(&u64::try_from(self.rule).unwrap_or(u64::MAX).to_le_bytes());
        material.extend_from_slice(&u64::try_from(index).unwrap_or(u64::MAX).to_le_bytes());
        Ok(ScriptCall {
            phase: if self.mode == Mode::After {
                ScriptPhase::AfterResponse
            } else {
                ScriptPhase::BeforeResponse
            },
            script: script.clone(),
            request: request_json(&self.context),
            vars: self.vars_map(),
            now: self.context.now,
            seed: whl_script::RandomSeed::from_bytes(&material),
            remaining,
            tenant: self.tenant.clone(),
        })
    }

    fn absorb_script(
        &mut self,
        result: Result<ScriptOutput, ScriptError>,
    ) -> Result<Done, Failure> {
        let output = result.map_err(|error| {
            Failure::new(FailureCode::Script, format!("{}: {error}", error.code()))
        })?;
        let responded = output.response.is_some();
        let names: Vec<&String> = output.vars.keys().collect();
        let summary = json!({
            "vars": names,
            "response": responded,
            "operations": output.stats.operations,
            "log": bound_text(&output.log.text, MAX_LOGGED_SCRIPT_BYTES),
        });
        self.context.vars = output.vars.into_iter().collect();
        if let Some(response) = output.response {
            self.apply_script_response(response);
        }
        Ok(Done::ok(summary))
    }

    fn apply_script_response(&mut self, response: ScriptResponse) {
        let headers = response
            .headers
            .into_iter()
            .map(|header| (header.name, header.value))
            .collect();
        self.response.replace(
            response.status.unwrap_or(DEFAULT_SCRIPT_STATUS),
            response.content_type,
            headers,
            response.body.unwrap_or_default().into_bytes(),
        );
    }

    fn absorb_outbound(&mut self, result: Result<OutboundDone, Failure>) -> Result<Done, Failure> {
        let done = result?;
        self.step_data = done.data;
        Ok(Done {
            outcome: done.outcome,
            detail: done.detail,
            output: Some(done.output),
        })
    }

    fn absorb_store(
        &mut self,
        result: Result<StoreValue, StateError>,
        written: Option<(String, Value)>,
    ) -> Result<Done, Failure> {
        match result {
            Ok(StoreValue::Written) => {
                let Some((name, value)) = written else {
                    return Err(Failure::new(
                        FailureCode::Internal,
                        "the written variable is unknown",
                    ));
                };
                let output = json!({"name": name, "value": logged_value(&value)});
                self.remember_store(&name, value);
                Ok(Done::ok(output))
            }
            Ok(StoreValue::Counter(value)) => {
                let name = match self.current_counter() {
                    Some(name) => name,
                    None => {
                        return Err(Failure::new(
                            FailureCode::Internal,
                            "the incremented counter is unknown",
                        ));
                    }
                };
                self.remember_store(&name, Value::from(value));
                Ok(Done::ok(json!({"counter": name, "value": value})))
            }
            Err(error) => Err(state_failure(error)),
        }
    }

    fn current_counter(&self) -> Option<String> {
        let rule = self.rules.rules().get(self.rule)?;
        match &rule.steps.get(self.step)?.action {
            CompiledAction::Increment { counter, .. } => Some(counter.clone()),
            _ => None,
        }
    }

    fn defer(
        &mut self,
        rule: &CompiledRule,
        index: usize,
        step: &CompiledStep,
    ) -> Result<Done, Failure> {
        let action = serde_json::to_value(&step.source).map_err(|_| {
            Failure::new(
                FailureCode::Internal,
                "the step cannot be encoded for later",
            )
        })?;
        let current = Snapshot {
            vars: self.vars_map(),
            payload: self.payload.clone(),
        };
        if self.snapshots.last() != Some(&current) {
            self.snapshots.push(current);
        }
        let snapshot = self.snapshots.len() - 1;
        self.deferred.push(DeferredStep {
            rule_id: rule.id,
            rule_name: rule.name.clone(),
            step: index,
            action,
            snapshot,
        });
        Ok(Done::plain(Outcome::Deferred, "runs after the response"))
    }

    fn conclude(
        &mut self,
        rule: &CompiledRule,
        index: usize,
        step: &CompiledStep,
        elapsed: Duration,
        result: Result<Done, Failure>,
    ) {
        let duration_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
        let (outcome, detail, output, error) = match result {
            Ok(done) => (done.outcome, done.detail, done.output, None),
            Err(failure) => {
                let entry = (
                    Outcome::Failed,
                    Some(failure.detail.clone()),
                    None,
                    Some(failure.code.as_str().to_owned()),
                );
                let forgiven = step.continue_on_error && failure.code != FailureCode::Budget;
                if !forgiven {
                    self.failure = Some(failure);
                }
                entry
            }
        };
        let data = self.step_data.take();
        if let Some(name) = &step.name {
            let mut data = data.unwrap_or_default();
            data.duration_ms = duration_ms;
            data.error = error;
            self.context.steps.insert(name.clone(), data);
        }
        if self.mode == Mode::After {
            self.checkpoint_due = true;
        }
        if self.mode != Mode::DryRun {
            metrics::counter!(
                RULE_STEPS_TOTAL,
                "kind" => step.kind.as_str(),
                "outcome" => outcome.as_str()
            )
            .increment(1);
        }
        self.log.push(Entry::Step(StepEntry {
            rule_id: rule.id,
            step: index,
            name: step.name.clone(),
            kind: step.kind.as_str(),
            outcome,
            duration_ms,
            detail: detail.map(|text| bound_text(&text, MAX_DETAIL_BYTES)),
            output: output.map(bound_output),
        }));
    }
}

fn logged(text: &str) -> String {
    bound_text(text, MAX_LOGGED_VALUE_BYTES)
}

fn logged_value(value: &Value) -> Value {
    match value {
        Value::String(text) => Value::String(logged(text)),
        other => other.clone(),
    }
}

fn parse_status(text: &str) -> Option<HttpStatus> {
    if text.len() != 3 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    HttpStatus::new(text.parse().ok()?)
}

fn store_value(text: &str) -> Value {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let canonical = !digits.is_empty()
        && digits.len() <= MAX_INTEGER_DIGITS
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && text != "-0";
    match text.parse::<i64>() {
        Ok(number) if canonical => Value::from(number),
        _ => Value::String(text.to_owned()),
    }
}

fn state_failure(error: StateError) -> Failure {
    match error {
        StateError::Unavailable | StateError::Timeout => Failure::new(
            FailureCode::Internal,
            "the variable store did not answer in time",
        ),
        StateError::EndpointGone => {
            Failure::new(FailureCode::Internal, "the endpoint is no longer available")
        }
        StateError::Rejected => Failure::new(
            FailureCode::Template,
            "the variable store rejected the value",
        ),
        StateError::LimitReached => Failure::new(
            FailureCode::Template,
            "the endpoint already holds the maximum number of variables",
        ),
        StateError::NotACounter => {
            Failure::new(FailureCode::Template, "the variable is not a counter")
        }
    }
}

fn first_values(pairs: &[NameValue], lowercase: bool) -> Map<String, Value> {
    let mut map = Map::new();
    for pair in pairs {
        let key = if lowercase {
            pair.name.to_ascii_lowercase()
        } else {
            pair.name.clone()
        };
        map.entry(key)
            .or_insert_with(|| Value::String(pair.value.clone()));
    }
    map
}

pub fn request_json(context: &Context) -> Value {
    let request = &context.request;
    json!({
        "method": request.method,
        "path": request.path,
        "segments": request.segments,
        "query": first_values(&request.query, false),
        "headers": first_values(&request.headers, true),
        "form": first_values(&request.form, false),
        "body": request.body.text(),
        "ip": request.ip,
        "content_type": request.content_type,
        "kind": request.kind,
        "meta": first_values(&request.meta, false),
        "signature": request.signature.as_str(),
        "capture": {
            "id": context.capture.id,
            "seq": context.capture.seq,
            "received_at": context.capture.received_at.format(&Rfc3339).unwrap_or_default(),
        },
        "endpoint": {
            "id": context.endpoint.id,
            "url": context.endpoint.url,
            "title": context.endpoint.title,
        },
        "store": context.store,
        "counter": context.counters,
    })
}
