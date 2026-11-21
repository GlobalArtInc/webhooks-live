use std::cell::Cell;
use std::collections::BTreeSet;
use std::time::Duration;

use time::macros::datetime;
use whl_template::{
    BodyData, Context, ErrorCode, MonotonicClock, Policy, RandomSeed, RandomSeedError,
    RandomSource, RenderEnv, RenderErrorCode, RenderLimits, RenderOutcome, RenderTarget,
    ResponseBudget, StepData, Template,
};

use crate::common::{
    ScriptedClock, SequenceRandom, SettableClock, SteppingClock, diagnostics_in, fixture, parse_in,
    render, render_in, render_template,
};

fn millis(count: u64) -> Duration {
    Duration::from_millis(count)
}

fn render_budgeted(
    template: &Template,
    limits: &RenderLimits,
    clock: &dyn MonotonicClock,
    budget: &ResponseBudget,
) -> RenderOutcome {
    let mut random = SequenceRandom::counting();
    let mut env = RenderEnv::new(&mut random, clock);
    template.render_within(&fixture(), limits, &mut env, budget)
}

fn first_code(source: &str, target: RenderTarget) -> Option<ErrorCode> {
    diagnostics_in(source, &Policy::outbound(target))
        .first()
        .map(|diagnostic| diagnostic.code)
}

#[test]
fn a_render_that_starts_with_no_budget_left_evaluates_nothing() {
    let clock = SettableClock::default();
    let budget = ResponseBudget::start(&clock, millis(50));
    clock.advance(millis(50));
    let template = parse_in("a{{ request.method }}b", RenderTarget::Body);
    let outcome = render_budgeted(
        &template,
        &RenderLimits::for_target(RenderTarget::Body),
        &clock,
        &budget,
    );
    assert!(!outcome.is_clean());
    assert_eq!(outcome.output(), "");
    assert_eq!(outcome.errors().len(), 1);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::BudgetExhausted);
    let partial = outcome.into_result().expect_err("partial");
    assert_eq!(partial.evaluations, 0);
}

#[test]
fn twenty_two_renders_share_one_budget() {
    let clock = SettableClock::default();
    let budget = ResponseBudget::start(&clock, millis(50));
    let template = parse_in("{{ request.method }}", RenderTarget::Body);
    let limits = RenderLimits::for_target(RenderTarget::Body);
    let mut clean = 0;
    let mut exhausted = 0;
    for _ in 0..22 {
        let outcome = render_budgeted(&template, &limits, &clock, &budget);
        if outcome.is_clean() {
            clean += 1;
        } else {
            assert_eq!(outcome.errors()[0].code, RenderErrorCode::BudgetExhausted);
            exhausted += 1;
        }
        clock.advance(millis(5));
    }
    assert_eq!((clean, exhausted), (10, 12));
    assert_eq!(budget.remaining(&clock), Duration::ZERO);
}

#[test]
fn the_remaining_budget_becomes_the_render_deadline() {
    let clock = SteppingClock::new(millis(10));
    let budget = ResponseBudget::start(&clock, millis(20));
    let template = parse_in(
        "{{ request.method }}{{ request.method }}{{ request.method }}",
        RenderTarget::Body,
    );
    let outcome = render_budgeted(
        &template,
        &RenderLimits::for_target(RenderTarget::Body),
        &clock,
        &budget,
    );
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::BudgetExhausted);
    assert_eq!(outcome.output(), "");
}

#[test]
fn the_per_render_deadline_stays_an_upper_bound_under_a_large_budget() {
    let clock = SteppingClock::new(millis(10));
    let budget = ResponseBudget::start(&clock, Duration::from_secs(1));
    let template = parse_in(
        "{{ request.method }}{{ request.method }}{{ request.method }}",
        RenderTarget::Body,
    );
    let mut limits = RenderLimits::for_target(RenderTarget::Body);
    limits.deadline = millis(15);
    let outcome = render_budgeted(&template, &limits, &clock, &budget);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::DeadlineExceeded);
    assert_eq!(outcome.output(), "POST");
}

#[test]
fn a_backwards_clock_never_refunds_the_budget() {
    let clock = ScriptedClock::new(vec![Duration::ZERO, millis(40), Duration::ZERO, millis(20)]);
    let budget = ResponseBudget::start(&clock, millis(50));
    assert_eq!(budget.remaining(&clock), millis(10));
    assert_eq!(budget.remaining(&clock), millis(10));
    assert_eq!(budget.remaining(&clock), Duration::ZERO);
}

#[test]
fn a_backwards_clock_does_not_disable_the_render_deadline() {
    let mut readings = vec![Duration::from_secs(1), Duration::ZERO];
    readings.extend((1..=10).map(|step| millis(step * 20)));
    let clock = ScriptedClock::new(readings);
    let template = parse_in(&"{{ request.method }}".repeat(8), RenderTarget::Body);
    let mut random = SequenceRandom::counting();
    let mut env = RenderEnv::new(&mut random, &clock);
    let outcome = template.render(
        &fixture(),
        &RenderLimits::for_target(RenderTarget::Body),
        &mut env,
    );
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::DeadlineExceeded);
    assert_eq!(outcome.output(), "POSTPOSTPOST");
}

#[test]
fn extreme_dates_with_an_offset_report_an_error_instead_of_panicking() {
    let mut context = fixture();
    context.capture.received_at = datetime!(9999-12-31 23:59:59 -23:59:59);
    for source in [
        "{{ capture.received_at }}",
        "{{ capture.received_at | date(\"%Y\") }}",
    ] {
        let outcome = render_in(source, RenderTarget::Body, &context);
        assert_eq!(
            outcome.errors()[0].code,
            RenderErrorCode::InvalidArgument,
            "{source}"
        );
    }
    context.capture.received_at = datetime!(2024-03-05 10:00:00 +02:00);
    let outcome = render_in("{{ capture.received_at }}", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "2024-03-05T08:00:00Z");
}

fn base64_of(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_with_limit(encoded: &str, max: usize) -> RenderOutcome {
    let mut context = fixture();
    context
        .vars
        .insert("b".to_owned(), serde_json::json!(encoded));
    let template = parse_in("{{ var.b | base64_decode }}", RenderTarget::Body);
    let mut limits = RenderLimits::for_target(RenderTarget::Body);
    limits.max_value_bytes = max;
    render_template(&template, &context, &limits)
}

#[test]
fn base64_decode_enforces_the_value_limit_on_its_output() {
    let outcome = decode_with_limit(&base64_of(&[b'a'; 100]), 16);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::ValueTooLarge);
    let outcome = decode_with_limit(&base64_of(&[b'a'; 16]), 16);
    assert_eq!(outcome.output(), "a".repeat(16));
    assert!(outcome.is_clean());
}

#[test]
fn base64_decode_enforces_the_value_limit_after_lossy_conversion() {
    let outcome = decode_with_limit(&base64_of(&[0xff; 10]), 16);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::ValueTooLarge);
    let outcome = decode_with_limit(&base64_of(&[0xff; 5]), 16);
    assert_eq!(outcome.output(), "\u{fffd}".repeat(5));
}

fn secret_context(token: &str, format: &str) -> Context {
    let mut context = fixture();
    context.secrets = whl_template::Secrets::new(
        [
            ("TOKEN".to_owned(), token.to_owned()),
            ("FMT".to_owned(), format.to_owned()),
        ]
        .into_iter()
        .collect(),
    );
    context
}

#[test]
fn partial_and_clean_renders_never_print_secret_derived_output() {
    let context = secret_context("hunter2-secret", "%Q");
    let partial = render_in(
        "{{ secret.TOKEN }}{{ now | date(secret.FMT) }}",
        RenderTarget::Body,
        &context,
    );
    assert_eq!(partial.output(), "hunter2-secret");
    let debug = format!("{partial:?}");
    assert!(!debug.contains("hunter2"), "{debug}");
    assert!(debug.contains("PartialRender"));
    let clean = render_in("{{ secret.TOKEN | upper }}", RenderTarget::Body, &context);
    assert!(clean.is_clean());
    let debug = format!("{clean:?}");
    assert!(
        !debug.contains("HUNTER2") && !debug.contains("hunter2"),
        "{debug}"
    );
    let rendered = clean.into_result().expect("clean");
    assert!(!format!("{rendered:?}").contains("HUNTER2"));
}

#[test]
fn the_context_debug_output_omits_the_secrets_map() {
    let context = secret_context("hunter2-secret", "%Q");
    let debug = format!("{context:?}");
    assert!(!debug.contains("hunter2"));
    assert!(!debug.contains("TOKEN"));
    assert!(debug.contains("redacted"));
}

#[test]
fn a_bad_date_format_from_a_secret_is_not_echoed() {
    let context = secret_context("unused", "%\u{3a9}");
    let outcome = render_in("{{ now | date(secret.FMT) }}", RenderTarget::Body, &context);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidArgument);
    for error in outcome.errors() {
        assert!(!error.message.contains('\u{3a9}'), "{}", error.message);
    }
    assert!(!format!("{outcome:?}").contains('\u{3a9}'));
}

#[test]
fn literal_bad_date_formats_still_name_the_specifier_at_save_time() {
    let diagnostics = diagnostics_in(
        "{{ now | date(\"%Q\") }}",
        &Policy::outbound(RenderTarget::Body),
    );
    assert_eq!(diagnostics[0].code, ErrorCode::InvalidDateFormat);
    assert!(diagnostics[0].message.contains("%Q"));
}

#[test]
fn names_outside_the_allowed_pattern_are_rejected_at_save_time() {
    let rejected = [
        "{{ counter.Foo }}".to_owned(),
        "{{ var[\"a b\"] }}".to_owned(),
        "{{ var.Foo }}".to_owned(),
        "{{ store.bad-name }}".to_owned(),
        "{{ counter[\"x y\"] }}".to_owned(),
        "{{ var.1abc }}".to_owned(),
        format!("{{{{ var.{} }}}}", "a".repeat(65)),
    ];
    for source in rejected {
        assert_eq!(
            first_code(&source, RenderTarget::Body),
            Some(ErrorCode::UnknownName),
            "{source}"
        );
    }
    let accepted = [
        "{{ counter.refunds }}".to_owned(),
        "{{ var._x9 }}".to_owned(),
        "{{ store.a_b_c }}".to_owned(),
        "{{ var.user.Name }}".to_owned(),
        format!("{{{{ var.{} }}}}", "a".repeat(64)),
    ];
    for source in accepted {
        assert_eq!(first_code(&source, RenderTarget::Body), None, "{source}");
    }
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn derived_seeds_are_an_hmac_of_the_run_id_under_the_server_key() {
    let seed = RandomSeed::derive(b"Jefe", b"what do ya want for nothing?").expect("seed");
    assert_eq!(
        hex_of(seed.unpack()),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn derived_seeds_depend_on_both_the_key_and_the_run_id() {
    let base = RandomSeed::derive(b"server-key", b"run-1").expect("seed");
    assert_eq!(
        base,
        RandomSeed::derive(b"server-key", b"run-1").expect("seed")
    );
    assert_ne!(
        base,
        RandomSeed::derive(b"other-key", b"run-1").expect("seed")
    );
    assert_ne!(
        base,
        RandomSeed::derive(b"server-key", b"run-2").expect("seed")
    );
    assert_ne!(base, RandomSeed::from_bytes(b"run-1"));
    assert_eq!(format!("{base:?}"), "RandomSeed(..)");
}

#[test]
fn an_empty_server_key_is_refused() {
    assert_eq!(
        RandomSeed::derive(b"", b"run-1").expect_err("refused"),
        RandomSeedError::EmptyKey
    );
}

fn context_with_body(body: &str) -> Context {
    let mut context = fixture();
    context.request.body = BodyData::from_text(body);
    context
}

fn padded(json: &str, total: usize) -> String {
    format!("{json}{}", " ".repeat(total - json.len()))
}

#[test]
fn json_bodies_above_the_default_cap_make_lookups_missing_with_an_error() {
    assert_eq!(
        RenderLimits::for_target(RenderTarget::Body).max_json_body_bytes,
        256 * 1024
    );
    let at_cap = context_with_body(&padded("{\"a\":1}", 256 * 1024));
    let outcome = render_in("[{{ json(\"$.a\") }}]", RenderTarget::Body, &at_cap);
    assert!(outcome.is_clean());
    assert_eq!(outcome.output(), "[1]");
    let over = context_with_body(&padded("{\"a\":1}", 256 * 1024 + 1));
    let outcome = render_in("[{{ json(\"$.a\") }}]", RenderTarget::Body, &over);
    assert_eq!(outcome.output(), "[]");
    assert_eq!(outcome.errors().len(), 1);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::JsonBodyTooLarge);
    assert_eq!(outcome.missing().len(), 1);
}

#[test]
fn the_json_body_cap_is_configurable_but_never_above_one_mebibyte() {
    let template = parse_in("[{{ json(\"$.a\") }}]", RenderTarget::Body);
    let mut limits = RenderLimits::for_target(RenderTarget::Body);
    limits.max_json_body_bytes = 4 * 1024 * 1024;
    let medium = context_with_body(&padded("{\"a\":1}", 300 * 1024));
    assert_eq!(render_template(&template, &medium, &limits).output(), "[1]");
    let huge = context_with_body(&padded("{\"a\":1}", 1024 * 1024 + 1));
    let outcome = render_template(&template, &huge, &limits);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::JsonBodyTooLarge);
    limits.max_json_body_bytes = 6;
    let outcome = render_template(&template, &context_with_body("{\"a\":1}"), &limits);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::JsonBodyTooLarge);
}

#[test]
fn step_bodies_obey_the_json_body_cap() {
    let mut context = fixture();
    context.steps.insert(
        "big".to_owned(),
        StepData {
            body: padded("{\"a\":1}", 256 * 1024 + 1),
            ..StepData::default()
        },
    );
    let outcome = render_in(
        "[{{ json(step.big.body, \"$.a\") }}]",
        RenderTarget::Body,
        &context,
    );
    assert_eq!(outcome.output(), "[]");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::JsonBodyTooLarge);
}

#[test]
fn a_leading_utf8_bom_is_ignored_before_json_parsing() {
    let context = context_with_body("\u{feff}{\"a\":7}");
    let outcome = render_in("{{ json(\"$.a\") }}", RenderTarget::Body, &context);
    assert!(outcome.is_clean());
    assert_eq!(outcome.output(), "7");
}

struct ConstantRandom {
    byte: u8,
    calls: Cell<usize>,
}

impl RandomSource for ConstantRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.calls.set(self.calls.get() + 1);
        destination.fill(self.byte);
    }
}

#[test]
fn random_int_gives_up_rejection_sampling_after_sixty_four_draws() {
    let template = parse_in("{{ random_int(1, 6) }}", RenderTarget::Body);
    let mut random = ConstantRandom {
        byte: 0xff,
        calls: Cell::new(0),
    };
    let clock = SettableClock::default();
    let mut env = RenderEnv::new(&mut random, &clock);
    let outcome = template.render(
        &fixture(),
        &RenderLimits::for_target(RenderTarget::Body),
        &mut env,
    );
    assert!(outcome.is_clean());
    assert_eq!(outcome.output(), "4");
    assert_eq!(random.calls.get(), 64);
}

#[test]
fn the_json_filter_is_rejected_in_a_json_string_target() {
    let diagnostics = diagnostics_in(
        "\"{{ var.n | json }}\"",
        &Policy::outbound(RenderTarget::JsonString),
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, ErrorCode::JsonFilterInJsonString);
    assert!(diagnostics[0].message.contains("json filter"));
    assert_eq!(first_code("{{ var.n | json }}", RenderTarget::Body), None);
    assert_eq!(
        first_code("{{ var.n | upper }}", RenderTarget::JsonString),
        None
    );
}

#[test]
fn a_missing_counter_renders_zero_and_is_recorded() {
    let outcome = render("{\"seen\":{{ counter.refunds }},\"hits\":{{ counter.hits }}}");
    assert!(outcome.is_clean());
    assert_eq!(outcome.output(), "{\"seen\":0,\"hits\":3}");
    let parsed: serde_json::Value = serde_json::from_str(outcome.output()).expect("json");
    assert_eq!(parsed["seen"], 0);
    assert_eq!(outcome.missing().len(), 1);
    assert_eq!(outcome.missing()[0].reference, "counter.refunds");
    assert_eq!(outcome.missing()[0].position.column, 12);
}

#[test]
fn a_missing_counter_used_as_a_default_argument_is_recorded() {
    let outcome = render("{{ var.absent | default(counter.fallback) }}");
    assert_eq!(outcome.output(), "0");
    assert_eq!(outcome.missing().len(), 1);
    assert_eq!(outcome.missing()[0].reference, "counter.fallback");
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn templates_list_the_counters_vars_and_secrets_they_reference() {
    let template = parse_in(
        "{{ var.a }}{{ var.b.c | default(counter.x) }}{{ secret.K | upper }}{{ random_int(var.lo, 5) }}\
         {{ request.method | replace(\"a\", var.r) }}{{ counter.y | date(secret.F) }}{{ store.s }}\
         {{ var.a | hmac_sha256(secret.K) }}",
        RenderTarget::Body,
    );
    assert_eq!(template.referenced_counters(), names(&["x", "y"]));
    assert_eq!(template.referenced_vars(), names(&["a", "b", "lo", "r"]));
    assert_eq!(template.referenced_secrets(), names(&["K", "F"]));
    assert_eq!(template.referenced_store(), names(&["s"]));
    let plain = parse_in("{{ request.method }}", RenderTarget::Body);
    assert!(plain.referenced_counters().is_empty());
    assert!(plain.referenced_vars().is_empty());
    assert!(plain.referenced_secrets().is_empty());
    assert!(plain.referenced_store().is_empty());
}
