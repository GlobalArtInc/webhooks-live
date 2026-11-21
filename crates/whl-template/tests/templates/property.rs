use proptest::prelude::*;
use whl_template::{BodyData, Policy, RenderEnv, RenderLimits, RenderTarget, Template, validate};

use crate::common::{FixedClock, SequenceRandom, fixture, pairs, parse_in, render_template};

const FRAGMENTS: &[&str] = &[
    "{{",
    "}}",
    "{{ ",
    " }}",
    "|",
    "\"",
    "\\",
    "(",
    ")",
    ",",
    "[",
    "]",
    ".",
    "-",
    "é",
    "日本",
    "\u{1f600}",
    "\n",
    "\r\n",
    "\0",
    " ",
    "text ",
    "request",
    "request.method",
    "request.query.name",
    "request.header.x-token",
    "request.body",
    "request.headers",
    "var.user.tags[1]",
    "var.user.name",
    "var.absent",
    "secret.TOKEN",
    "step.lookup.body",
    "upper",
    "lower",
    "trim",
    "length",
    "json",
    "url",
    "html",
    "base64",
    "base64_decode",
    "hex",
    "sha256",
    "default(\"d\")",
    "truncate(3)",
    "replace(\"a\", \"bb\")",
    "hmac_sha256(\"k\")",
    "date(\"%Y\")",
    "uuid()",
    "random_hex(8)",
    "random_int(1, 9)",
    "json(\"$.order.id\")",
    "{{ request.method }}",
    "{{ request.query.name | upper | url }}",
    "{{ var.user.name | replace(\"a\", \"xyz\") | hex }}",
    "{{ json(\"$..sku\") | default(\"none\") }}",
    "{{ capture.received_at | date(\"%F\") }}",
    "{{ \"{{\" }}",
    "{{ 42 }}",
];

fn template_source() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(FRAGMENTS), 0..40).prop_map(|parts| parts.concat())
}

const PRIMARIES: &[&str] = &[
    "request.method",
    "request.query.name",
    "request.body",
    "request.headers",
    "request.header.x-token",
    "var.user.tags[1]",
    "var.user",
    "var.absent",
    "capture.received_at",
    "capture.seq",
    "now",
    "json(\"$.order\")",
    "json(\"$..sku\")",
    "uuid()",
    "random_hex(12)",
    "random_int(-5, 5)",
    "\"literal {{ text\"",
    "42",
    "secret.TOKEN",
];

const FILTERS: &[&str] = &[
    "upper",
    "lower",
    "trim",
    "length",
    "json",
    "url",
    "html",
    "base64",
    "base64_decode",
    "hex",
    "sha256",
    "default(\"d\")",
    "default(var.n)",
    "truncate(5)",
    "replace(\"a\", \"bbbb\")",
    "hmac_sha256(\"k\")",
    "hmac_sha256(secret.TOKEN)",
    "date(\"%Y-%m-%d %H\")",
];

fn expression() -> impl Strategy<Value = String> {
    (
        prop::sample::select(PRIMARIES),
        prop::collection::vec(prop::sample::select(FILTERS), 0..5),
    )
        .prop_map(|(primary, filters)| {
            let chain: String = filters
                .iter()
                .map(|filter| format!(" | {filter}"))
                .collect();
            format!("{{{{ {primary}{chain} }}}}")
        })
}

fn structured_source() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        expression(),
        prop::sample::select(vec!["text ", "é", "}}", "{", "\r\n", " "]).prop_map(str::to_owned),
    ];
    prop::collection::vec(piece, 0..30).prop_map(|parts| parts.concat())
}

fn arbitrary_source() -> impl Strategy<Value = String> {
    prop_oneof![
        structured_source(),
        structured_source(),
        template_source(),
        any::<String>(),
        "[{}| \"a-z.\\[\\]()0-9\\\\]{0,200}"
    ]
}

fn targets() -> impl Strategy<Value = RenderTarget> {
    prop::sample::select(vec![
        RenderTarget::Body,
        RenderTarget::HeaderValue,
        RenderTarget::Status,
        RenderTarget::JsonString,
    ])
}

fn config() -> ProptestConfig {
    ProptestConfig {
        cases: 300,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn parsing_never_panics_and_diagnostics_are_bounded(source in arbitrary_source(), target in targets()) {
        let policy = Policy::outbound(target);
        match Template::parse(&source, &policy) {
            Ok(template) => prop_assert!(template.expression_count() <= 500),
            Err(error) => {
                prop_assert!(!error.diagnostics.is_empty());
                prop_assert!(error.diagnostics.len() <= 50);
                for diagnostic in &error.diagnostics {
                    prop_assert!(diagnostic.position.line >= 1);
                    prop_assert!(diagnostic.position.column >= 1);
                    prop_assert!(diagnostic.position.offset <= source.len());
                }
            }
        }
    }

    #[test]
    fn rendering_never_panics_and_respects_every_cap(
        source in arbitrary_source(),
        target in targets(),
        output_cap in 0usize..300,
        evaluations in 0usize..40,
        lookups in 0usize..20,
        query in any::<String>(),
        body in prop::collection::vec(any::<u8>(), 0..200),
    ) {
        let Ok(template) = Template::parse(&source, &Policy::outbound(target)) else {
            return Ok(());
        };
        let mut context = fixture();
        context.request.query = pairs(&[("name", &query)]);
        context.request.body = BodyData::new(body);
        let mut limits = RenderLimits::for_target(target);
        limits.max_output_bytes = output_cap.min(limits.max_output_bytes);
        limits.max_evaluations = evaluations;
        limits.max_json_lookups = lookups;
        limits.max_value_bytes = 4096;
        let outcome = render_template(&template, &context, &limits);
        prop_assert!(outcome.output().len() <= limits.max_output_bytes);
        prop_assert!(outcome.errors().len() <= limits.max_errors + 1);
        match &outcome {
            whl_template::RenderOutcome::Clean(rendered) => prop_assert!(rendered.evaluations <= evaluations),
            whl_template::RenderOutcome::Partial(partial) => prop_assert!(partial.evaluations <= evaluations),
        }
        match target {
            RenderTarget::HeaderValue => {
                prop_assert!(outcome.output().chars().all(|c| c == '\t' || (' '..='~').contains(&c)));
            }
            RenderTarget::Status => {
                prop_assert!(outcome.output().chars().all(|c| c.is_ascii_digit()));
                if outcome.is_clean() {
                    prop_assert_eq!(outcome.output().len(), 3);
                }
            }
            RenderTarget::Body | RenderTarget::JsonString => {}
        }
    }

    #[test]
    fn substituted_values_are_always_output_verbatim(value in any::<String>()) {
        let template = parse_in("<{{ request.query.name }}>", RenderTarget::Body);
        let mut context = fixture();
        context.request.query = pairs(&[("name", &value)]);
        let outcome = render_template(&template, &context, &RenderLimits::for_target(RenderTarget::Body));
        prop_assert!(outcome.is_clean());
        prop_assert_eq!(outcome.output(), format!("<{value}>"));
    }

    #[test]
    fn header_output_never_contains_line_breaks_or_nul(value in any::<String>()) {
        let template = parse_in("x{{ request.query.name }}y", RenderTarget::HeaderValue);
        let mut context = fixture();
        context.request.query = pairs(&[("name", &value)]);
        let outcome = render_template(&template, &context, &RenderLimits::for_target(RenderTarget::HeaderValue));
        prop_assert!(!outcome.output().contains(['\r', '\n', '\0']));
        let safe = value.chars().all(|c| c == '\t' || (' '..='~').contains(&c));
        prop_assert_eq!(outcome.is_clean(), safe && value.len() + 2 <= 1024);
    }

    #[test]
    fn json_string_target_output_stays_a_single_json_string(value in any::<String>()) {
        let template = parse_in("\"{{ request.query.name }}\"", RenderTarget::JsonString);
        let mut context = fixture();
        context.request.query = pairs(&[("name", &value)]);
        let outcome = render_template(&template, &context, &RenderLimits::for_target(RenderTarget::JsonString));
        prop_assert!(outcome.is_clean());
        let parsed: serde_json::Value = serde_json::from_str(outcome.output()).expect("json string");
        prop_assert_eq!(parsed, serde_json::Value::String(value));
    }

    #[test]
    fn unicode_before_an_error_gives_character_columns(prefix in "[^{}\n]{0,40}") {
        let source = format!("{prefix}{{{{ nope }}}}");
        let error = Template::parse(&source, &Policy::outbound(RenderTarget::Body)).expect_err("invalid");
        let expected = u32::try_from(prefix.chars().count() + 4).expect("fits");
        prop_assert_eq!(error.diagnostics[0].position.column, expected);
        prop_assert_eq!(error.diagnostics[0].position.line, 1);
    }

    #[test]
    fn repeated_substitution_never_exceeds_the_cap(repeat in 1usize..60, cap in 1usize..5000) {
        let source = "{{ var.text | hex }}".repeat(repeat);
        let template = parse_in(&source, RenderTarget::Body);
        let mut limits = RenderLimits::for_target(RenderTarget::Body);
        limits.max_output_bytes = cap;
        let outcome = render_template(&template, &fixture(), &limits);
        prop_assert!(outcome.output().len() <= cap);
    }
}

#[test]
fn pathological_nesting_is_rejected_without_panicking() {
    let cases = [
        "{{".repeat(20_000),
        "{{ ".repeat(5_000) + &"}}".repeat(5_000),
        format!("{{{{ request{} }}}}", "[".repeat(10_000)),
        format!("{{{{ var.a{} }}}}", "[0]".repeat(10_000)),
        format!("{{{{ request.method{} }}}}", " | ".repeat(10_000)),
        format!("{{{{ json(\"{}\") }}}}", "(".repeat(1_000)),
        format!(
            "{{{{ json(\"{}\") }}}}",
            "$[?(@.a".to_owned() + &")".repeat(100)
        ),
        format!("{{{{ \"{}", "\\".repeat(10_000)),
        format!("{{{{ f({}) }}}}", "1,".repeat(10_000)),
        "\u{0}".repeat(1000),
        "}}{{".repeat(10_000),
    ];
    for source in cases {
        let _ = validate(&source, &Policy::outbound(RenderTarget::Body));
        let _ = validate(&source, &Policy::response(RenderTarget::HeaderValue));
    }
}

#[test]
fn huge_repeated_substitutions_finish_within_the_caps() {
    let source = "{{ request.body }}".repeat(500);
    let template = parse_in(&source, RenderTarget::Body);
    let mut context = fixture();
    context.request.body = BodyData::new(vec![b'z'; 100_000]);
    let outcome = render_template(
        &template,
        &context,
        &RenderLimits::for_target(RenderTarget::Body),
    );
    assert!(outcome.output().len() <= 1024 * 1024);
    assert!(!outcome.is_clean());
}

#[test]
fn random_seeds_and_clocks_do_not_affect_safety() {
    let template = parse_in(
        "{{ uuid() }}{{ random_hex(64) }}{{ random_int(1, 9) }}",
        RenderTarget::Body,
    );
    let mut random = SequenceRandom::new(vec![0xff, 0, 0x80, 0x7f]);
    let clock = FixedClock;
    let mut env = RenderEnv::new(&mut random, &clock);
    let outcome = template.render(
        &fixture(),
        &RenderLimits::for_target(RenderTarget::Body),
        &mut env,
    );
    assert!(outcome.is_clean());
}
