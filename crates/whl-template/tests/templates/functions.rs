use whl_template::{BodyData, RenderErrorCode, RenderLimits, RenderTarget};

use crate::common::{
    SequenceRandom, fixture, parse_in, render, render_clean, render_in, render_template,
};

#[test]
fn json_reads_the_first_match_of_a_path() {
    let cases = [
        ("{{ json(\"$.order.id\") }}", "42"),
        ("{{ json(\"$.order.items[1].sku\") }}", "B2"),
        ("{{ json(\"$.order.items[*].sku\") }}", "A1"),
        ("{{ json(\"$..sku\") }}", "A1"),
        ("{{ json(\"$.name\") }}", "Zoë"),
        (
            "{{ json(\"$.order\") }}",
            r#"{"id":42,"items":[{"sku":"A1"},{"sku":"B2"}]}"#,
        ),
        (
            "{{ json(\"$.order.items\") }}",
            r#"[{"sku":"A1"},{"sku":"B2"}]"#,
        ),
        ("{{ json(\"$.order.items[?@.sku == 'B2'].sku\") }}", "B2"),
    ];
    for (source, expected) in cases {
        assert_eq!(render_clean(source), expected, "{source}");
    }
}

#[test]
fn json_misses_render_empty_and_take_defaults() {
    let outcome = render("[{{ json(\"$.absent\") }}][{{ json(\"$.nothing\") }}]");
    assert_eq!(outcome.output(), "[][]");
    assert_eq!(outcome.missing().len(), 2);
    assert_eq!(
        render_clean("{{ json(\"$.absent\") | default(\"fallback\") }}"),
        "fallback"
    );
}

#[test]
fn json_on_a_non_json_body_is_missing_not_an_error() {
    let mut context = fixture();
    context.request.body = BodyData::from_text("not json at all");
    let outcome = render_in("[{{ json(\"$.a\") }}]", RenderTarget::Body, &context);
    assert!(outcome.is_clean());
    assert_eq!(outcome.output(), "[]");
}

#[test]
fn json_ignores_bodies_nested_deeper_than_64() {
    let mut context = fixture();
    let deep = |levels: usize| format!("{}1{}", "[".repeat(levels), "]".repeat(levels));
    context.request.body = BodyData::from_text(&deep(65));
    let outcome = render_in("{{ json(\"$[0][0]\") }}", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "");
    context.request.body = BodyData::from_text(&deep(64));
    let outcome = render_in(
        "{{ json(\"$[0][0]\") | length }}",
        RenderTarget::Body,
        &context,
    );
    assert_eq!(outcome.output(), "125");
}

#[test]
fn json_ignores_bodies_larger_than_one_mebibyte() {
    let mut context = fixture();
    let padding = " ".repeat(1024 * 1024);
    context.request.body = BodyData::from_text(&format!("{padding}{{\"a\":1}}"));
    let outcome = render_in("[{{ json(\"$.a\") }}]", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "[]");
    context.request.body = BodyData::from_text(&format!("{{\"a\":1}}{}", " ".repeat(1000)));
    let outcome = render_in("[{{ json(\"$.a\") }}]", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "[1]");
}

#[test]
fn json_reads_a_step_body() {
    assert_eq!(
        render_clean("{{ json(step.lookup.body, \"$.data.v\") }}"),
        "1"
    );
    let outcome = render("[{{ json(step.other.body, \"$.a\") }}]");
    assert_eq!(outcome.output(), "[]");
}

#[test]
fn uuid_is_a_version_four_value_from_the_random_source() {
    assert_eq!(
        render_clean("{{ uuid() }}"),
        "00010203-0405-4607-8809-0a0b0c0d0e0f"
    );
}

#[test]
fn random_hex_returns_the_requested_number_of_characters() {
    let template = parse_in(
        "{{ random_hex(5) }}|{{ random_hex(64) | length }}",
        RenderTarget::Body,
    );
    let mut random = SequenceRandom::new(vec![0xab, 0xcd, 0xef]);
    let clock = crate::common::FixedClock;
    let mut env = whl_template::RenderEnv::new(&mut random, &clock);
    let outcome = template.render(
        &fixture(),
        &RenderLimits::for_target(RenderTarget::Body),
        &mut env,
    );
    assert_eq!(outcome.output(), "abcde|64");
}

#[test]
fn random_hex_rejects_runtime_lengths_out_of_range() {
    let mut context = fixture();
    context.vars.insert("big".to_owned(), serde_json::json!(65));
    let outcome = render_in("[{{ random_hex(var.big) }}]", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "[]");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidArgument);
}

fn random_int_with(source: &str, bytes: Vec<u8>) -> String {
    let template = parse_in(source, RenderTarget::Body);
    let mut random = SequenceRandom::new(bytes);
    let clock = crate::common::FixedClock;
    let mut env = whl_template::RenderEnv::new(&mut random, &clock);
    template
        .render(
            &fixture(),
            &RenderLimits::for_target(RenderTarget::Body),
            &mut env,
        )
        .output()
        .to_owned()
}

#[test]
fn random_int_stays_inside_the_inclusive_range() {
    assert_eq!(random_int_with("{{ random_int(1, 6) }}", vec![0; 8]), "1");
    assert_eq!(
        random_int_with("{{ random_int(1, 6) }}", vec![5, 0, 0, 0, 0, 0, 0, 0]),
        "6"
    );
    assert_eq!(
        random_int_with("{{ random_int(-5, -5) }}", vec![9; 8]),
        "-5"
    );
    assert_eq!(random_int_with("{{ random_int(-3, 3) }}", vec![0; 8]), "-3");
}

#[test]
fn random_int_rejects_biased_draws() {
    let mut bytes = vec![0xff; 8];
    bytes.extend([4, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(random_int_with("{{ random_int(1, 6) }}", bytes), "5");
}

#[test]
fn random_int_handles_the_full_i64_range() {
    let mut context = fixture();
    context
        .vars
        .insert("lo".to_owned(), serde_json::json!("-9223372036854775808"));
    context
        .vars
        .insert("hi".to_owned(), serde_json::json!("9223372036854775807"));
    let template = parse_in("{{ random_int(var.lo, var.hi) }}", RenderTarget::Body);
    let outcome = render_template(
        &template,
        &context,
        &RenderLimits::for_target(RenderTarget::Body),
    );
    assert!(outcome.is_clean());
    assert!(outcome.output().parse::<i64>().is_ok());
}

#[test]
fn random_int_rejects_an_inverted_runtime_range() {
    let outcome = render("{{ random_int(var.n, 1) }}");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidArgument);
    let outcome = render("{{ random_int(var.user.name, 1) }}");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::TypeMismatch);
}
