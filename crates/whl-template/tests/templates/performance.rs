use std::time::{Duration, Instant};

use whl_template::{RenderEnv, RenderLimits, RenderTarget, SystemClock};

use crate::common::{SequenceRandom, fixture, parse_in};

fn typical_template() -> String {
    let substitutions = [
        "{{ request.method }}",
        "{{ request.path }}",
        "{{ request.query.name | upper }}",
        "{{ request.header.x-token | default(\"none\") }}",
        "{{ capture.id }}",
        "{{ capture.seq }}",
        "{{ var.user.name | truncate(10) }}",
        "{{ json(\"$.order.id\") }}",
        "{{ json(\"$.order.items[1].sku\") | lower }}",
        "{{ endpoint.url | url }}",
        "{{ now | date(\"%Y-%m-%d\") }}",
        "{{ request.ip }}",
        "{{ store.s | json }}",
        "{{ counter.hits }}",
        "{{ request.query.a | length }}",
        "{{ request.form.f | html }}",
        "{{ var.text | sha256 | truncate(16) }}",
        "{{ request.content_type }}",
        "{{ request.kind }}",
        "{{ var.n }}",
    ];
    let filler = "lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor ";
    let mut source = String::new();
    for substitution in substitutions {
        source.push_str(filler);
        source.push_str(substitution);
    }
    while source.len() < 2048 {
        source.push('.');
    }
    source
}

#[test]
fn rendering_a_typical_template_one_hundred_thousand_times_stays_linear() {
    let source = typical_template();
    assert!(source.len() >= 2048);
    let template = parse_in(&source, RenderTarget::Body);
    let context = fixture();
    let mut limits = RenderLimits::for_target(RenderTarget::Body);
    limits.deadline = Duration::from_secs(30);
    let clock = SystemClock::new();
    let mut random = SequenceRandom::counting();
    let started = Instant::now();
    let mut total = 0usize;
    for _ in 0..100_000 {
        let mut env = RenderEnv::new(&mut random, &clock);
        let outcome = template.render(&context, &limits, &mut env);
        assert!(outcome.is_clean(), "{:?}", outcome.errors());
        total += outcome.output().len();
    }
    let elapsed = started.elapsed();
    assert!(total > 100_000 * 1000);
    assert!(
        elapsed < Duration::from_secs(300),
        "100000 renders took {elapsed:?}"
    );
}

#[test]
fn a_maximal_template_parses_in_linear_time() {
    let source = "{{ request.method | upper | lower | trim }}".repeat(400);
    let started = Instant::now();
    let template = parse_in(&source, RenderTarget::Body);
    assert_eq!(template.expression_count(), 400);
    assert!(started.elapsed() < Duration::from_secs(60));
}

#[test]
fn error_positions_on_a_huge_single_line_stay_cheap() {
    let source = "{{ nope }}".repeat(50) + &"x".repeat(60_000);
    let started = Instant::now();
    let error =
        whl_template::Template::parse(&source, &whl_template::Policy::outbound(RenderTarget::Body))
            .expect_err("invalid");
    assert_eq!(error.diagnostics.len(), 50);
    assert!(started.elapsed() < Duration::from_secs(60));
}
