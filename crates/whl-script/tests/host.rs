mod common;

use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use time::macros::datetime;
use whl_script::{
    MAX_HEADER_NAME_BYTES, MAX_HEADER_VALUE_BYTES, MAX_RESPONSE_HEADERS, RandomSeed, RunLimits,
    ScriptError, ScriptResponse, SeededRandom,
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
fn direct_assignments_to_the_request_are_refused() {
    for source in ["request.method = \"GET\"; ()", "request = 5;"] {
        let text = failure(source).to_string();
        assert!(text.contains("constant"), "{source}: {text}");
    }
}

#[test]
fn mutating_or_shadowing_the_request_inside_the_script_never_reaches_the_host() {
    let pool = pool(1, 4);
    let original = default_request();
    let probe = "#{ vars: #{ method: request.method, event: request.headers[\"x-event\"], count: request.headers.len() } }";
    for source in [
        "request.headers[\"e\"] = \"1\"; ()",
        "request.headers.extra = 1; ()",
        "request.headers.clear(); ()",
        "request.headers[\"e\"] = \"1\"; let request = #{ method: \"POST\" }; ()",
        "let request = #{ forged: true }; #{ vars: #{ seen: request } }",
    ] {
        let first = pool.run(job_with(compile(source), original.clone(), roomy()));
        if let Err(error) = &first {
            assert_eq!(error.code(), "runtime", "{source}");
        }
        let fresh = pool
            .run(job_with(compile(probe), original.clone(), roomy()))
            .expect("a fresh run sees the pristine request");
        assert_eq!(fresh.vars["method"], json!("POST"), "{source}");
        assert_eq!(fresh.vars["event"], json!("push"), "{source}");
        assert_eq!(fresh.vars["count"], json!(1), "{source}");
    }
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
        r#"#{ response: #{ headers: [#{ name: "X-One", value: "a=1" }, #{ name: "X-Two", value: "b=2" }] } }"#,
    )
    .expect("run");
    let response = output.response.expect("response");
    assert_eq!(response.headers.len(), 2);
    assert_eq!(response.status, None);
}

#[test]
fn a_structurally_broken_response_is_refused_with_a_named_error() {
    for source in [
        r#"#{ response: #{ status: "200" } }"#,
        "#{ response: #{ body: 5 } }",
        "#{ response: #{ unknown: 1 } }",
        "#{ response: 5 }",
        "#{ response: #{ headers: 5 } }",
        "#{ other: 1 }",
        "5",
        r#""text""#,
    ] {
        assert_eq!(failure(source).code(), "invalid_result", "{source}");
    }
}

fn violations(source: &str) -> Vec<(String, &'static str)> {
    match failure(source) {
        ScriptError::InvalidResponse(found) => found
            .into_iter()
            .map(|violation| (violation.field, violation.code))
            .collect(),
        other => panic!("{source}: expected invalid_response, got {other:?}"),
    }
}

#[test]
fn response_violations_follow_the_core_rules_and_are_reported_together() {
    let source = r#"#{ response: #{ status: 101, content_type: "nonsense", headers: [
        #{ name: "Set-Cookie", value: "a=1" },
        #{ name: "Content-Length", value: "5" },
        #{ name: "Transfer-Encoding", value: "chunked" },
        #{ name: "x\"y", value: "v" },
        #{ name: "A", value: "1" },
        #{ name: "a", value: "2" },
        #{ name: "X-Value", value: "line\nbreak" }] } }"#;
    let found = violations(source);
    let has = |field: &str, code: &str| {
        found
            .iter()
            .any(|(name, kind)| name == field && *kind == code)
    };
    assert!(
        has("response.content_type", "invalid_media_type"),
        "{found:?}"
    );
    assert!(has("response.headers[0].name", "denied"), "{found:?}");
    assert!(has("response.headers[1].name", "denied"), "{found:?}");
    assert!(has("response.headers[2].name", "denied"), "{found:?}");
    assert!(
        has("response.headers[3].name", "invalid_token"),
        "{found:?}"
    );
    assert!(has("response.headers[5].name", "duplicate"), "{found:?}");
    assert!(
        has("response.headers[6].value", "control_character"),
        "{found:?}"
    );
}

#[test]
fn duplicate_names_in_a_header_map_are_caught_case_insensitively() {
    let found = violations(r#"#{ response: #{ headers: #{ "A": "1", "a": "2" } } }"#);
    assert_eq!(
        found,
        vec![("response.headers[1].name".to_owned(), "duplicate")]
    );
}

#[test]
fn informational_and_out_of_range_statuses_follow_the_core_status_rule() {
    let output = run(&pool(1, 4), "#{ response: #{ status: 101 } }").expect("101 passes");
    assert_eq!(output.response.expect("response").status, Some(101));
    for status in [99, 600, 0, -1, 70000] {
        let found = violations(&format!("#{{ response: #{{ status: {status} }} }}"));
        assert_eq!(found, vec![("response.status".to_owned(), "out_of_range")]);
    }
}

const DENIED_HEADER_NAMES: &[&str] = &[
    "Content-Length",
    "Transfer-Encoding",
    "Connection",
    "Keep-Alive",
    "Upgrade",
    "Trailer",
    "TE",
    "Host",
    "Set-Cookie",
    "Set-Cookie2",
    "X-Content-Type-Options",
    "X-Request-Id",
    "X-Endpoint-Id",
    "Content-Type",
    "Clear-Site-Data",
    "Strict-Transport-Security",
    "Cache-Control",
    "Alt-Svc",
    "Refresh",
    "Content-Encoding",
    "X-Frame-Options",
    "Referrer-Policy",
    "Permissions-Policy",
    "Service-Worker-Allowed",
    "Public-Key-Pins",
    "Public-Key-Pins-Report-Only",
    "Report-To",
    "NEL",
    "Origin-Trial",
    "Document-Policy",
    "WWW-Authenticate",
    "Proxy-Authenticate",
    "Accept-CH",
    "Critical-CH",
    "Set-Login",
    "Proxy-Authorization",
    "Content-Security-Policy",
    "Content-Security-Policy-Report-Only",
    "Access-Control-Allow-Origin",
    "Cross-Origin-Opener-Policy",
];

#[test]
fn every_denylisted_header_name_is_refused_in_any_case() {
    let pool = pool(1, 4);
    for name in DENIED_HEADER_NAMES {
        for spelled in [
            (*name).to_owned(),
            name.to_ascii_lowercase(),
            name.to_ascii_uppercase(),
        ] {
            let source = format!(r#"#{{ response: #{{ headers: #{{ "{spelled}": "v" }} }} }}"#);
            match run(&pool, &source) {
                Err(ScriptError::InvalidResponse(found)) => {
                    assert_eq!(found.len(), 1, "{spelled}");
                    assert_eq!(found[0].code, "denied", "{spelled}");
                }
                other => panic!("{spelled}: {other:?}"),
            }
        }
    }
}

#[test]
fn response_size_and_count_limits_follow_the_core_rules() {
    let pool = pool(1, 4);
    let many: String = (0..=MAX_RESPONSE_HEADERS)
        .map(|index| format!(r#""h{index}": "v","#))
        .collect();
    let found = violations(&format!("#{{ response: #{{ headers: #{{ {many} }} }} }}"));
    assert_eq!(found, vec![("response.headers".to_owned(), "too_many")]);
    let long_name = "n".repeat(MAX_HEADER_NAME_BYTES + 1);
    let long_value = "v".repeat(MAX_HEADER_VALUE_BYTES + 1);
    let source = format!(
        r#"#{{ response: #{{ headers: #{{ "{long_name}": "v", "ok": "{long_value}" }} }} }}"#
    );
    let found = violations(&source);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|(_, code)| *code == "too_long"));
    let allowed = run(
        &pool,
        r#"#{ response: #{ status: 200, content_type: "application/json; charset=utf-8", headers: #{ "X-Ok": "fine" } } }"#,
    )
    .expect("a valid response passes");
    assert_eq!(allowed.response.expect("response").status, Some(200));
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
        .run(job_with(compile("()"), json!([1, 2]), roomy()))
        .expect_err("not an object");
    assert_eq!(error.code(), "invalid_input");
    let limits = RunLimits {
        max_input_bytes: 100,
        ..roomy()
    };
    let error = pool
        .run(job_with(
            compile("()"),
            json!({"body": "x".repeat(200)}),
            limits,
        ))
        .expect_err("too large");
    assert_eq!(error.code(), "input_too_large");
    let mut deep = json!(1);
    for _ in 0..50 {
        deep = json!({ "k": deep });
    }
    let error = pool
        .run(job_with(compile("()"), deep, roomy()))
        .expect_err("too deep");
    assert_eq!(error.code(), "input_too_large");
}

fn array_request(items: usize) -> serde_json::Value {
    json!({ "arr": (0..items).collect::<Vec<_>>() })
}

#[test]
fn a_large_input_array_is_usable_and_an_oversized_one_is_refused_before_the_script_runs() {
    let pool = pool(1, 4);
    for items in [299, 3000, 4096] {
        let output = pool
            .run(job_with(
                compile(
                    "let copy = request.arr; #{ vars: #{ n: request.arr.len(), m: copy.len() } }",
                ),
                array_request(items),
                roomy(),
            ))
            .expect("the array is readable");
        assert_eq!(output.vars["n"], json!(items));
        assert_eq!(output.vars["m"], json!(items));
    }
    let error = pool
        .run(job_with(
            compile("try { request.arr.len() } catch { 0 }"),
            array_request(5000),
            roomy(),
        ))
        .expect_err("too many items");
    assert_eq!(error.code(), "input_too_large");
    assert!(error.to_string().contains("4096"), "{error}");
}

#[test]
fn an_input_map_key_longer_than_the_cap_is_refused_before_the_script_runs() {
    let pool = pool(1, 4);
    let mut request = default_request();
    request["headers"] = json!({ "k".repeat(256): "ok" });
    pool.run(job_with(compile("()"), request.clone(), roomy()))
        .expect("a key at the cap is accepted");
    request["headers"] = json!({ "k".repeat(257): "too long" });
    let error = pool
        .run(job_with(compile("()"), request, roomy()))
        .expect_err("key above the cap");
    assert_eq!(error.code(), "input_too_large");
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
            roomy(),
        ))
        .expect("readable");
    assert_eq!(output.vars["n"], json!(40_000));
    let error = pool
        .run(job_with(
            compile("let b = request.body + request.body; 1"),
            request,
            RunLimits {
                time: Duration::from_secs(5),
                ..roomy()
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
        roomy(),
    );
    assert!(!format!("{input:?}").contains("top-secret"));
}
