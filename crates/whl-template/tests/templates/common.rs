use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::json;
use time::OffsetDateTime;
use whl_template::{
    BodyData, Context, Diagnostic, MonotonicClock, NameValue, Policy, RandomSource, RenderEnv,
    RenderLimits, RenderOutcome, RenderTarget, SignatureState, StepData, Template,
};

pub struct FixedClock;

impl MonotonicClock for FixedClock {
    fn elapsed(&self) -> Duration {
        Duration::ZERO
    }
}

pub struct SteppingClock {
    step: Duration,
    calls: Cell<u32>,
}

impl SteppingClock {
    pub fn new(step: Duration) -> Self {
        Self {
            step,
            calls: Cell::new(0),
        }
    }
}

impl MonotonicClock for SteppingClock {
    fn elapsed(&self) -> Duration {
        self.calls.set(self.calls.get() + 1);
        self.step * self.calls.get()
    }
}

#[derive(Default)]
pub struct SettableClock {
    now: Cell<Duration>,
}

impl SettableClock {
    pub fn advance(&self, by: Duration) {
        self.now.set(self.now.get() + by);
    }
}

impl MonotonicClock for SettableClock {
    fn elapsed(&self) -> Duration {
        self.now.get()
    }
}

pub struct ScriptedClock {
    readings: Vec<Duration>,
    calls: Cell<usize>,
}

impl ScriptedClock {
    pub fn new(readings: Vec<Duration>) -> Self {
        Self {
            readings,
            calls: Cell::new(0),
        }
    }
}

impl MonotonicClock for ScriptedClock {
    fn elapsed(&self) -> Duration {
        let index = self.calls.get();
        self.calls.set(index + 1);
        self.readings
            .get(index)
            .or(self.readings.last())
            .copied()
            .unwrap_or(Duration::ZERO)
    }
}

pub struct SequenceRandom {
    bytes: Vec<u8>,
    position: usize,
}

impl SequenceRandom {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, position: 0 }
    }

    pub fn counting() -> Self {
        Self::new((0..=255).collect())
    }
}

impl RandomSource for SequenceRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        for slot in destination {
            *slot = self.bytes[self.position % self.bytes.len()];
            self.position += 1;
        }
    }
}

pub const NOW_UNIX: i64 = 1_709_647_629;

pub fn fixture() -> Context {
    let now = OffsetDateTime::from_unix_timestamp(NOW_UNIX).expect("valid timestamp");
    let mut context = Context::new(now);
    let request = &mut context.request;
    request.method = "POST".to_owned();
    request.path = "/hook/orders".to_owned();
    request.segments = vec!["hook".to_owned(), "orders".to_owned()];
    request.query = pairs(&[("a", "1"), ("a", "2"), ("name", "Zoë & co")]);
    request.headers = pairs(&[
        ("Content-Type", "application/json"),
        ("X-Token", "abc"),
        ("X-Token", "def"),
    ]);
    request.form = pairs(&[("f", "form value")]);
    request.body = BodyData::from_text(
        r#"{"order":{"id":42,"items":[{"sku":"A1"},{"sku":"B2"}]},"name":"Zoë","nothing":null}"#,
    );
    request.ip = "203.0.113.9".to_owned();
    request.content_type = "application/json".to_owned();
    request.kind = "http".to_owned();
    request.meta = pairs(&[("k", "v")]);
    request.signature = SignatureState::Valid;
    context.capture.id = "cap_1".to_owned();
    context.capture.seq = 7;
    context.endpoint.id = "ep_1".to_owned();
    context.endpoint.url = "https://webhooks.live/e/ep_1".to_owned();
    context.endpoint.title = "Orders".to_owned();
    context.vars = BTreeMap::from([
        (
            "user".to_owned(),
            json!({"name": "ann", "tags": ["x", "y"], "n": 5, "ok": true}),
        ),
        ("n".to_owned(), json!(5)),
        ("padded".to_owned(), json!("  hi  ")),
        (
            "text".to_owned(),
            json!("The quick brown fox jumps over the lazy dog"),
        ),
        ("empty".to_owned(), json!("")),
        ("nul".to_owned(), json!(null)),
    ]);
    context.store = BTreeMap::from([("s".to_owned(), json!("stored"))]);
    context.counters = BTreeMap::from([("hits".to_owned(), 3)]);
    context.steps = BTreeMap::from([(
        "lookup".to_owned(),
        StepData {
            status: Some(201),
            headers: pairs(&[("X-Id", "9")]),
            body: r#"{"data":{"v":1}}"#.to_owned(),
            duration_ms: 12,
            error: None,
        },
    )]);
    context.secrets =
        whl_template::Secrets::new(BTreeMap::from([("TOKEN".to_owned(), "s3cret".to_owned())]));
    context
}

pub fn pairs(items: &[(&str, &str)]) -> Vec<NameValue> {
    items
        .iter()
        .map(|(name, value)| NameValue::new(*name, *value))
        .collect()
}

pub fn parse_in(source: &str, target: RenderTarget) -> Template {
    Template::parse(source, &Policy::outbound(target)).expect("template parses")
}

pub fn diagnostics_in(source: &str, policy: &Policy) -> Vec<Diagnostic> {
    match Template::parse(source, policy) {
        Ok(_) => Vec::new(),
        Err(error) => error.diagnostics,
    }
}

pub fn render_template(
    template: &Template,
    context: &Context,
    limits: &RenderLimits,
) -> RenderOutcome {
    let mut random = SequenceRandom::counting();
    let clock = FixedClock;
    let mut env = RenderEnv::new(&mut random, &clock);
    template.render(context, limits, &mut env)
}

pub fn render_in(source: &str, target: RenderTarget, context: &Context) -> RenderOutcome {
    let template = parse_in(source, target);
    render_template(&template, context, &RenderLimits::for_target(target))
}

pub fn render(source: &str) -> RenderOutcome {
    render_in(source, RenderTarget::Body, &fixture())
}

pub fn render_clean(source: &str) -> String {
    let outcome = render(source);
    assert!(
        outcome.is_clean(),
        "unexpected errors for {source:?}: {:?}",
        outcome.errors()
    );
    outcome.output().to_owned()
}
