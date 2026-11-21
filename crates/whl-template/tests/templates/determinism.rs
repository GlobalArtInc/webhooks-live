use whl_template::{ChaChaRandom, RandomSeed, RenderEnv, RenderLimits, RenderTarget};

use crate::common::{FixedClock, fixture, parse_in};

fn render_seeded(seed: &[u8]) -> String {
    let template = parse_in(
        "{{ uuid() }} {{ random_hex(16) }} {{ random_int(1, 1000000) }} {{ now | date(\"%s\") }}",
        RenderTarget::Body,
    );
    let mut random = ChaChaRandom::from_seed(&RandomSeed::from_bytes(seed));
    let clock = FixedClock;
    let mut env = RenderEnv::new(&mut random, &clock);
    let outcome = template.render(
        &fixture(),
        &RenderLimits::for_target(RenderTarget::Body),
        &mut env,
    );
    assert!(outcome.is_clean());
    outcome.output().to_owned()
}

#[test]
fn the_same_seed_renders_the_same_output() {
    assert_eq!(render_seeded(b"run-1"), render_seeded(b"run-1"));
}

#[test]
fn different_seeds_render_different_output() {
    assert_ne!(render_seeded(b"run-1"), render_seeded(b"run-2"));
}

#[test]
fn repeated_renders_of_one_template_are_identical_without_randomness() {
    let template = parse_in(
        "{{ request.method }} {{ capture.received_at }} {{ request.headers }}",
        RenderTarget::Body,
    );
    let context = fixture();
    let first = crate::common::render_template(
        &template,
        &context,
        &RenderLimits::for_target(RenderTarget::Body),
    );
    let second = crate::common::render_template(
        &template,
        &context,
        &RenderLimits::for_target(RenderTarget::Body),
    );
    assert_eq!(first, second);
}

#[test]
fn seeds_are_not_exposed_through_debug() {
    let seed = RandomSeed::from_bytes(b"secret-run-id");
    assert_eq!(format!("{seed:?}"), "RandomSeed(..)");
    assert_eq!(seed.unpack().len(), 32);
}
