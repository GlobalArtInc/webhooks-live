mod common;

use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use time::macros::datetime;
use whl_script::{
    MAX_RESPONSE_HEADERS, RandomSeed, RunLimits, ScriptError, ScriptResponse, SeededRandom,
};

fn vars(source: &str) -> Value {
    let pool = pool(1, 4);
    let wrapped = format!("#{{ vars: #{{ out: {source} }} }}");
    vars_of(&run(&pool, &wrapped).expect("run succeeds"))["out"].clone()
}

fn failure(source: &str) -> ScriptError {
    run(&pool(1, 4), source).expect_err("the run must fail")
}

#[test]
fn request_fields_are_readable_and_missing_ones_are_unit() {
    assert_eq!(vars("request.method"), json!("POST"));
    assert_eq!(vars("request.headers[\"x-event\"]"), json!("push"));
    assert_eq!(vars("request.nothing"), Value::Null);
    assert_eq!(vars("json_parse(request.body).a"), json!(1));
}

#[test]
fn the_request_is_read_only() {
    for source in [
        "request.method = \"GET\"; ()",
        "request.headers.extra = 1; ()",
        "request.headers[\"x-event\"] = \"other\"; ()",
    ] {
        let error = failure(source);
        assert_eq!(error.code(), "runtime", "{source}");
        let text = error.to_string();
        assert!(
            text.contains("read-only request") || text.contains("constant"),
            "{source}: {text}"
        );
    }
    assert_eq!(failure("request = 5;").code(), "runtime");
    assert_eq!(failure("request.headers.clear(); ()").code(), "runtime");
}

#[test]
fn copies_of_the_request_may_be_changed_freely() {
    assert_eq!(
        vars("{ let copy = request.headers; copy.extra = 1; copy.extra }"),
        json!(1)
    );
}

#[test]
fn input_vars_are_visible_and_mutations_are_returned() {
    let pool = pool(1, 4);
    let mut input = job(compile("vars.count += 1; vars.added = \"yes\"; ()"));
    input.vars = serde_json::from_value(json!({"count": 41, "kept": true})).expect("map");
    let output = pool.run(input).expect("run succeeds");
    assert_eq!(
        vars_of(&output),
        json!({"count": 42, "kept": true, "added": "yes"})
    );
    assert!(output.response.is_none());
}

#[test]
fn a_returned_vars_map_replaces_the_scope_variable() {
    let pool = pool(1, 4);
    let output = run(&pool, "vars.ignored = 1; #{ vars: #{ only: 2 } }").expect("run");
    assert_eq!(vars_of(&output), json!({"only": 2}));
}

#[test]
fn vars_must_stay_a_map_of_json_values() {
    assert_eq!(failure("vars = 5; ()").code(), "invalid_result");
    assert_eq!(failure("#{ vars: #{ f: |x| x } }").code(), "invalid_result");
    assert_eq!(failure("#{ vars: 5 }").code(), "invalid_result");
    assert_eq!(
        failure("#{ vars: #{ n: 1.0 / 0.0 } }").code(),
        "invalid_result"
    );
}

#[test]
fn json_round_trips_through_the_host_functions() {
    assert_eq!(
        vars("json_parse(\"{\\\"a\\\":[1,2.5,null,true,\\\"x\\\"],\\\"b\\\":{}}\")"),
        json!({"a": [1, 2.5, null, true, "x"], "b": {}})
    );
    assert_eq!(
        vars("json_stringify(#{ list: [1, 2, #{ b: () }], s: \"q\" })"),
        json!("{\"list\":[1,2,{\"b\":null}],\"s\":\"q\"}")
    );
}

#[test]
fn json_errors_are_catchable_runtime_errors() {
    assert_eq!(
        vars("{ let r = 1; try { json_parse(\"{oops\"); } catch { r = 2; } r }"),
        json!(2)
    );
    let error = failure("json_parse(\"{oops\")");
    assert_eq!(error.code(), "runtime");
    assert!(error.to_string().contains("invalid_json"));
    let error = failure("json_stringify(|x| x)");
    assert!(error.to_string().contains("invalid_json"));
}

#[test]
fn base64_and_hex_follow_the_standard_vectors() {
    assert_eq!(vars("base64_encode(\"hello\")"), json!("aGVsbG8="));
    assert_eq!(vars("base64_decode(\"aGVsbG8=\")"), json!("hello"));
    assert_eq!(vars("hex(\"hi\")"), json!("6869"));
    let error = failure("base64_decode(\"!!!\")");
    assert!(error.to_string().contains("invalid_base64"));
    let error = failure("base64_decode(\"/w==\")");
    assert!(error.to_string().contains("not UTF-8"));
}

#[test]
fn hashing_matches_published_test_vectors() {
    assert_eq!(
        vars("sha256(\"abc\")"),
        json!("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
    );
    assert_eq!(
        vars("hmac_sha256(\"Jefe\", \"what do ya want for nothing?\")"),
        json!("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
    );
}

#[test]
fn regex_helpers_match_and_capture() {
    assert_eq!(
        vars("regex_match(\"^[a-z]+-\\\\d+$\", \"abc-42\")"),
        json!(true)
    );
    assert_eq!(vars("regex_match(\"^[a-z]+$\", \"abc-42\")"), json!(false));
    assert_eq!(
        vars("regex_captures(\"(\\\\w+)@(\\\\w+)\\\\.com\", \"bob@example.com\")"),
        json!(["bob@example.com", "bob", "example"])
    );
    assert_eq!(
        vars("regex_captures(\"(a)|(b)\", \"b\")"),
        json!(["b", null, "b"])
    );
    assert_eq!(vars("regex_captures(\"x\", \"abc\")"), json!([]));
}

#[test]
fn bad_regex_patterns_are_runtime_errors() {
    let error = failure("regex_match(\"(unclosed\", \"x\")");
    assert!(error.to_string().contains("invalid_regex"));
    let long = "a".repeat(600);
    let error = failure(&format!("regex_match(\"{long}\", \"x\")"));
    assert!(error.to_string().contains("pattern is too long"));
}

#[test]
fn uuid_and_random_int_come_from_the_injected_source() {
    let pool = pool(1, 4);
    let source = "#{ vars: #{ u: uuid(), r: random_int(10, 20), s: random_int(5, 5) } }";
    let first = pool.run(job(compile(source))).expect("run");
    let second = pool.run(job(compile(source))).expect("run");
    assert_eq!(vars_of(&first), vars_of(&second));
    let uuid = first.vars["u"].as_str().expect("uuid is text");
    assert_eq!(uuid.len(), 36);
    assert_eq!(uuid.as_bytes()[14], b'4');
    assert!(matches!(uuid.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    let drawn = first.vars["r"].as_i64().expect("integer");
    assert!((10..=20).contains(&drawn));
    assert_eq!(first.vars["s"], json!(5));

    let mut other = job(compile(source));
    other.random = Box::new(SeededRandom::from_seed(&RandomSeed::from_bytes(b"another")));
    let third = pool.run(other).expect("run");
    assert_ne!(third.vars["u"], first.vars["u"]);
}

#[test]
fn random_int_covers_its_inclusive_range_and_rejects_a_reversed_one() {
    let pool = pool(1, 4);
    let output = run(
        &pool,
        "let seen = #{}; for i in 0..200 { seen[random_int(1, 3).to_string()] = true; } #{ vars: seen }",
    )
    .expect("run");
    assert_eq!(vars_of(&output), json!({"1": true, "2": true, "3": true}));
    let error = failure("random_int(5, 1)");
    assert!(error.to_string().contains("lower bound"));
    assert!(vars("random_int(-9223372036854775807 - 1, 9223372036854775807)").is_i64());
}

#[test]
fn now_is_the_injected_capture_time() {
    let pool = pool(1, 4);
    let mut input = job(compile("#{ vars: #{ now: now() } }"));
    input.now = datetime!(2026-11-21 10:30:00.5 +03:00);
    let output = pool.run(input).expect("run");
    assert_eq!(output.vars["now"], json!("2026-11-21T07:30:00.5Z"));
}

#[test]
fn the_response_is_extracted_and_validated() {
    let pool = pool(1, 4);
    let output = run(
        &pool,
        "#{ response: #{ status: 202, body: \"queued\", content_type: \"text/plain\", headers: #{ \"X-A\": \"1\" } } }",
    )
    .expect("run");
    assert_eq!(
        output.response,
        Some(ScriptResponse {
            status: Some(202),
            headers: vec![whl_script::ResponseHeader {
                name: "X-A".to_owned(),
                value: "1".to_owned()
            }],
            body: Some("queued".to_owned()),
            content_type: Some("text/plain".to_owned()),
        })
    );
    let output = run(
        &pool,
        "#{ response: #{ headers: [#{ name: \"Set-Cookie\", value: \"a=1\" }, #{ name: \"Set-Cookie\", value: \"b=2\" }] } }",
    )
    .expect("run");
    let response = output.response.expect("response");
    assert_eq!(response.headers.len(), 2);
    assert_eq!(response.status, None);
}

#[test]
fn an_invalid_response_is_refused_with_a_named_error() {
    for source in [
        "#{ response: #{ status: 99 } }",
        "#{ response: #{ status: 600 } }",
        "#{ response: #{ status: \"200\" } }",
        "#{ response: #{ body: 5 } }",
        "#{ response: #{ unknown: 1 } }",
        "#{ response: 5 }",
        "#{ response: #{ headers: #{ \"X-A\": \"a\\nb\" } } }",
        "#{ response: #{ headers: #{ \"bad name\": \"a\" } } }",
        "#{ response: #{ headers: #{ \"\": \"a\" } } }",
        "#{ response: #{ content_type: \"a\\r\\nb\" } }",
        "#{ other: 1 }",
        "5",
        "\"text\"",
    ] {
        assert_eq!(failure(source).code(), "invalid_result", "{source}");
    }
    let many: String = (0..=MAX_RESPONSE_HEADERS)
        .map(|index| format!("\"h{index}\": \"v\","))
        .collect();
    let error = failure(&format!("#{{ response: #{{ headers: #{{ {many} }} }} }}"));
    assert_eq!(error.code(), "invalid_result");
}

#[test]
fn the_script_may_return_nothing() {
    let pool = pool(1, 4);
    let output = run(&pool, "let x = 1;").expect("run");
    assert!(output.response.is_none());
    assert!(output.vars.is_empty());
}

#[test]
fn printed_and_debugged_lines_reach_the_log() {
    let pool = pool(1, 4);
    let output = run(&pool, "print(\"first\"); print(42); debug(\"second\"); ()").expect("run");
    assert_eq!(output.log.text, "first\n42\n[debug line 1] \"second\"\n");
    assert!(!output.log.truncated);
    assert!(output.stats.operations > 0);
    assert!(output.stats.peak_memory_bytes > 0);
}

#[test]
fn inputs_that_are_not_objects_or_are_too_large_are_refused() {
    let pool = pool(1, 4);
    let error = pool
        .run(job_with(compile("()"), json!([1, 2]), RunLimits::default()))
        .expect_err("not an object");
    assert_eq!(error.code(), "invalid_input");
    let limits = RunLimits {
        max_input_bytes: 100,
        ..RunLimits::default()
    };
    let error = pool
        .run(job_with(
            compile("()"),
            json!({"body": "x".repeat(200)}),
            limits,
        ))
        .expect_err("too large");
    assert_eq!(error.code(), "invalid_input");
    let mut deep = json!(1);
    for _ in 0..50 {
        deep = json!({ "k": deep });
    }
    let error = pool
        .run(job_with(compile("()"), deep, RunLimits::default()))
        .expect_err("too deep");
    assert_eq!(error.code(), "invalid_input");
}

#[test]
fn a_large_input_string_is_readable_but_cannot_be_grown_past_the_string_limit() {
    let pool = pool(1, 4);
    let mut request = default_request();
    request["body"] = json!("z".repeat(40_000));
    let output = pool
        .run(job_with(
            compile("#{ vars: #{ n: request.body.len() } }"),
            request.clone(),
            RunLimits::default(),
        ))
        .expect("readable");
    assert_eq!(output.vars["n"], json!(40_000));
    let error = pool
        .run(job_with(
            compile("let b = request.body + request.body; 1"),
            request,
            RunLimits {
                time: Duration::from_secs(5),
                ..RunLimits::default()
            },
        ))
        .expect_err("grown");
    assert_eq!(error.code(), "data_too_large");
}

#[test]
fn secrets_stay_out_of_debug_output() {
    let seed = RandomSeed::from_bytes(b"very secret");
    assert_eq!(format!("{seed:?}"), "RandomSeed(..)");
    let random = SeededRandom::from_seed(&seed);
    assert_eq!(format!("{random:?}"), "SeededRandom(..)");
    let input = job_with(
        compile("()"),
        json!({"authorization": "Bearer top-secret"}),
        RunLimits::default(),
    );
    assert!(!format!("{input:?}").contains("top-secret"));
}
