use std::time::{Duration, Instant};

use proptest::prelude::*;
use whl_template::{
    BodyData, ErrorCode, Policy, RenderEnv, RenderErrorCode, RenderLimits, RenderTarget,
    SystemClock, Template,
};

use crate::common::{SequenceRandom, diagnostics_in, fixture, parse_in, render_in};

fn escape(path: &str) -> String {
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

fn source_for(path: &str) -> String {
    format!("{{{{ json(\"{}\") }}}}", escape(path))
}

fn code_of(path: &str) -> Option<ErrorCode> {
    diagnostics_in(&source_for(path), &Policy::outbound(RenderTarget::Body))
        .first()
        .map(|diagnostic| diagnostic.code)
}

fn accepted(path: &str) -> bool {
    code_of(path).is_none()
}

fn nested_body(levels: usize, items: usize) -> String {
    let filler = "0,".repeat(items);
    format!("{}{filler}0{}", "[".repeat(levels), "]".repeat(levels))
}

fn timed_render(
    path: &str,
    body: String,
    limits: &RenderLimits,
) -> (Duration, Vec<RenderErrorCode>) {
    let template = parse_in(&source_for(path), RenderTarget::Body);
    let mut context = fixture();
    context.request.body = BodyData::new(body.into_bytes());
    let clock = SystemClock::new();
    let mut random = SequenceRandom::counting();
    let mut env = RenderEnv::new(&mut random, &clock);
    let started = Instant::now();
    let outcome = template.render(&context, limits, &mut env);
    let codes = outcome.errors().iter().map(|error| error.code).collect();
    (started.elapsed(), codes)
}

#[test]
fn match_and_search_are_rejected_at_save_time() {
    let paths = [
        "$[?match(@, 'a')]",
        "$[?search(@.a, 'b')]",
        "$[?match(@, '\\\\w{100}')]",
        "$[?match(@.a, @.b)]",
        "$[?match (@, 'a')]",
        "$[?!search(@, 'a')]",
    ];
    for path in paths {
        let diagnostics = diagnostics_in(&source_for(path), &Policy::outbound(RenderTarget::Body));
        assert_eq!(diagnostics.len(), 1, "{path}");
        assert_eq!(diagnostics[0].code, ErrorCode::InvalidJsonPath, "{path}");
        assert_eq!(diagnostics[0].code.field_code(), "json_path_invalid");
        assert!(
            diagnostics[0].message.contains("match and search"),
            "{path}"
        );
    }
}

#[test]
fn the_regex_free_functions_keep_working() {
    let mut context = fixture();
    context.request.body = BodyData::from_text(r#"{"items":[{"t":"ab"},{"t":"abcd"}]}"#);
    let source = source_for("$.items[?length(@.t) > 2].t");
    assert_eq!(
        render_in(&source, RenderTarget::Body, &context).output(),
        "abcd"
    );
    let source = source_for("$[?count(@.*) > 1]");
    assert!(render_in(&source, RenderTarget::Body, &context).is_clean());
}

#[test]
fn match_and_search_text_inside_strings_and_names_is_allowed() {
    assert!(accepted("$['match(']"));
    assert!(accepted("$.match"));
    assert!(accepted("$[?@.search == 'match(x)']"));
}

#[test]
fn the_reviewer_union_probes_are_rejected() {
    for segments in [10usize, 12, 16, 28] {
        let path = format!("${}", "[0,0,0,0]".repeat(segments));
        assert_eq!(
            code_of(&path),
            Some(ErrorCode::JsonPathTooComplex),
            "{segments}"
        );
    }
    assert_eq!(
        code_of(&format!("${}", "[0,0,0,0]".repeat(28))).map(ErrorCode::field_code),
        Some("json_path_invalid")
    );
}

#[test]
fn unions_are_limited_to_three_selectors_in_total() {
    assert!(accepted("$[0,1,2]"));
    assert!(accepted("$['a','b']"));
    assert_eq!(code_of("$[0,1,2,3]"), Some(ErrorCode::JsonPathTooComplex));
    assert_eq!(code_of("$[0,1][2,3]"), Some(ErrorCode::JsonPathTooComplex));
    assert!(accepted("$[0,1].a[2]"));
}

#[test]
fn multiplying_segments_after_the_first_are_limited_to_two() {
    assert!(accepted("$[*][*][*]"));
    assert!(accepted("$.a[*][*]"));
    assert_eq!(code_of("$.a[*][*][*]"), Some(ErrorCode::JsonPathTooComplex));
    assert_eq!(
        code_of("$[*][*][*][*]"),
        Some(ErrorCode::JsonPathTooComplex)
    );
    assert_eq!(code_of("$.a.*.*.*"), Some(ErrorCode::JsonPathTooComplex));
    assert_eq!(
        code_of("$.a[0:2][0:2][0:2]"),
        Some(ErrorCode::JsonPathTooComplex)
    );
    assert_eq!(
        code_of("$.a[?@.b][?@.c]"),
        Some(ErrorCode::JsonPathTooComplex)
    );
    assert!(accepted("$.a.b.c.d.e.f.g[*]"));
}

#[test]
fn nested_wildcards_are_cheap_on_a_wide_body() {
    let body = format!("[{}]", vec![nested_body(1, 120); 120].join(","));
    let limits = RenderLimits::for_target(RenderTarget::Body);
    let (took, codes) = timed_render("$[*][*][*]", body, &limits);
    assert!(took < Duration::from_millis(500), "{took:?}");
    assert!(codes.is_empty() || codes == [RenderErrorCode::JsonNodeLimitExceeded]);
}

#[test]
fn recursive_descent_over_a_deep_body_is_cheap() {
    let limits = RenderLimits::for_target(RenderTarget::Body);
    for path in ["$..*", "$..[0]", "$..x", "$..[*,*,*]"] {
        let (took, _) = timed_render(path, nested_body(62, 100_000), &limits);
        assert!(took < Duration::from_millis(1500), "{path}: {took:?}");
    }
}

#[test]
fn comparison_and_logical_operators_are_limited_to_four_per_path() {
    assert!(accepted("$[?@.a==1&&@.b==2&&@.c]"));
    assert_eq!(
        code_of("$[?@.a==1&&@.b==2&&@.c==3]"),
        Some(ErrorCode::JsonPathTooComplex)
    );
    assert!(accepted("$[?@.a<=1||@.b>=2]"));
    assert_eq!(
        code_of("$[?@.a!=1||@.b<2||@.c>3]"),
        Some(ErrorCode::JsonPathTooComplex)
    );
    assert!(accepted("$[?@.a=='&&||==!=<>']"));
}

#[test]
fn the_reviewer_operator_probe_is_rejected() {
    let mut path = String::from("$..[?@==@");
    while path.len() + 6 < 249 {
        path.push_str("&&@==@");
    }
    path.push(']');
    assert!(path.len() > 200);
    assert_eq!(code_of(&path), Some(ErrorCode::JsonPathTooComplex));
}

#[test]
fn filters_may_not_hide_nested_queries_that_multiply() {
    let rejected = [
        "$[?@[?@.a]]",
        "$[?$..a==1]",
        "$[?@..a==1]",
        "$[?$[*]]",
        "$[?@[0,1]==1]",
        "$[?count(@.*) > count(@[*])]",
        "$[?@[0:2]]",
    ];
    for path in rejected {
        assert_eq!(code_of(path), Some(ErrorCode::JsonPathTooComplex), "{path}");
    }
    assert!(accepted("$[?@.*]"));
    assert!(accepted("$[?@.a == 'x']"));
}

#[test]
fn the_right_side_of_a_comparison_must_be_a_literal() {
    let rejected = [
        "$[?@==@]",
        "$[?@.a == $.b]",
        "$[?@.a != @.b]",
        "$[?length(@.a) == length(@.b)]",
        "$[?value(@.a) <= value(@.b)]",
        "$[?1 == @.a]",
    ];
    for path in rejected {
        assert_eq!(code_of(path), Some(ErrorCode::JsonPathTooComplex), "{path}");
    }
    let allowed = [
        "$[?@.a == 'x']",
        "$[?@.a == \"x\"]",
        "$[?@.a >= -1.5]",
        "$[?@.a != null]",
        "$[?@.a == true]",
        "$[?length(@.a) > 2]",
        "$[?@.a<=3]",
    ];
    for path in allowed {
        assert!(accepted(path), "{path}");
    }
}

#[test]
fn worst_case_allowed_paths_finish_within_the_slack_on_the_largest_body() {
    let limits = RenderLimits::for_target(RenderTarget::Body);
    let paths = [
        "$..[?@.a==1&&@.b==2&&@.c]",
        "$..[?@.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a==1||@.b.b.b.b.b.b.b.b.b.b==2]",
        "$..[?length(@)>0&&length(@)>1]",
        "$..[?count(@.*)>0&&@.a]",
        "$..[*,*,*]",
        "$[*][*][*]",
        "$..[0:100000]",
    ];
    for path in paths {
        for levels in [1usize, 31, 62] {
            let body = nested_body(levels, 120_000);
            assert!(body.len() <= limits.max_json_body_bytes);
            let (took, _) = timed_render(path, body, &limits);
            assert!(
                took < Duration::from_millis(1500),
                "{path} at {levels}: {took:?}"
            );
        }
    }
}

#[test]
fn chained_member_filters_stay_cheap_on_many_deep_chains() {
    let chain = format!("{}1{}", "{\"a\":".repeat(60), "}".repeat(60));
    let body = format!("[{}]", vec![chain; 600].join(","));
    assert!(body.len() <= 256 * 1024);
    let limits = RenderLimits::for_target(RenderTarget::Body);
    let path = "$..[?@.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a==1||@.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a.a==2]";
    let (took, _) = timed_render(path, body, &limits);
    assert!(took < Duration::from_millis(1500), "{took:?}");
}

#[test]
fn matches_beyond_the_node_cap_report_an_error() {
    let body = format!("[{}0]", "0,".repeat(1000));
    let limits = RenderLimits::for_target(RenderTarget::Body);
    let (_, codes) = timed_render("$[*]", body, &limits);
    assert_eq!(codes, [RenderErrorCode::JsonNodeLimitExceeded]);
    let body = format!("[{}0]", "0,".repeat(999));
    let (_, codes) = timed_render("$[*]", body, &limits);
    assert!(codes.is_empty());
    let mut tight = limits;
    tight.max_json_nodes = 2;
    let (_, codes) = timed_render("$[*]", "[1,2,3]".to_owned(), &tight);
    assert_eq!(codes, [RenderErrorCode::JsonNodeLimitExceeded]);
}

#[test]
fn the_node_cap_defaults_to_one_thousand() {
    assert_eq!(
        RenderLimits::for_target(RenderTarget::Body).max_json_nodes,
        1000
    );
}

#[test]
fn a_rejected_path_never_reaches_the_renderer() {
    let policy = Policy::outbound(RenderTarget::Body);
    let probe = format!("{{{{ json(\"${}\") }}}}", "[0,0,0,0]".repeat(28));
    assert!(Template::parse(&probe, &policy).is_err());
}

const PATH_FRAGMENTS: &[&str] = &[
    ".a",
    ".*",
    ".b.c",
    "[*]",
    "[0]",
    "[0,1]",
    "[0,1,2]",
    "[0:3]",
    "[::2]",
    "['a']",
    "['a','b']",
    "..a",
    "..*",
    "..[*]",
    "..[0]",
    "[?@.a==1]",
    "[?@.a==1&&@.b]",
    "[?@.a==1||@.b=='x'&&@.c]",
    "[?length(@)>1]",
    "[?count(@.*)>0]",
    "[?@.*]",
    "[?@==@]",
    "[?@..a==1]",
    "[?match(@,'a')]",
    "[?search(@,'a')]",
];

fn random_path() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(PATH_FRAGMENTS), 1..8)
        .prop_map(|parts| format!("${}", parts.concat()))
}

fn shaped_body() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        nested_body(1, 120_000),
        nested_body(31, 60_000),
        nested_body(62, 100_000),
        format!(
            "[{}]",
            vec!["{\"a\":{\"a\":[1,2,{\"b\":3}]}}"; 7000].join(",")
        ),
        "x".repeat(1024 * 1024),
        nested_body(1, 600_000),
    ])
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 40, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn accepted_paths_never_outlast_the_slack_on_large_bodies(path in random_path(), body in shaped_body()) {
        let policy = Policy::outbound(RenderTarget::Body);
        let source = source_for(&path);
        let Ok(template) = Template::parse(&source, &policy) else {
            return Ok(());
        };
        let mut context = fixture();
        context.request.body = BodyData::new(body.into_bytes());
        let limits = RenderLimits::for_target(RenderTarget::Body);
        let clock = SystemClock::new();
        let mut random = SequenceRandom::counting();
        let mut env = RenderEnv::new(&mut random, &clock);
        let started = Instant::now();
        let outcome = template.render(&context, &limits, &mut env);
        let took = started.elapsed();
        prop_assert!(took < limits.deadline + Duration::from_millis(1450), "{path}: {took:?}");
        prop_assert!(outcome.output().len() <= limits.max_output_bytes);
    }

    #[test]
    fn arbitrary_path_text_never_panics_the_preflight(path in "\\PC{0,300}") {
        let _ = diagnostics_in(&source_for(&path), &Policy::outbound(RenderTarget::Body));
    }
}
