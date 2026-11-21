mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use serde_json::json;
use whl_script::{
    EngineLimits, ErrorCode, LimitsError, RunLimits, ScriptCompiler, ScriptError, ScriptPhase,
};

fn failure(pool: &whl_script::ScriptPool, source: &str) -> ScriptError {
    run(pool, source).expect_err("the run must fail")
}

fn roomy() -> RunLimits {
    RunLimits {
        time: Duration::from_secs(20),
        ..RunLimits::default()
    }
}

#[test]
fn an_infinite_loop_is_stopped_by_the_operation_limit() {
    let pool = pool(1, 4);
    let error = failure(&pool, "while true {}");
    assert_eq!(error, ScriptError::Operations);
    assert_eq!(error.code(), "operations");
    assert_eq!(error.outcome(), "operations");
}

#[test]
fn try_catch_cannot_swallow_a_limit() {
    let pool = pool(1, 4);
    assert_eq!(
        failure(&pool, "try { loop {} } catch { 1 }"),
        ScriptError::Operations
    );
    assert_eq!(
        failure(&pool, "try { let s = \"a\"; loop { s += s; } } catch { 1 }").code(),
        "data_too_large"
    );
}

#[test]
fn deep_recursion_is_stopped_by_the_call_depth_limit() {
    let pool = pool(1, 4);
    let error = failure(&pool, "fn f(n) { f(n + 1) } f(0)");
    assert_eq!(error, ScriptError::CallDepth);
    assert_eq!(error.code(), "call_depth");
}

#[test]
fn recursion_inside_the_call_depth_limit_runs_on_the_worker_stack() {
    let pool = pool(1, 4);
    let output = run(
        &pool,
        "fn f(n) { if n == 0 { 0 } else { 1 + f(n - 1) } } #{ vars: #{ depth: f(30) } }",
    )
    .expect("within the limit");
    assert_eq!(vars_of(&output), json!({"depth": 30}));
    assert_eq!(
        failure(
            &pool,
            "fn f(n) { if n == 0 { 0 } else { 1 + f(n - 1) } } f(40)"
        ),
        ScriptError::CallDepth
    );
}

#[test]
fn the_deepest_allowed_recursion_with_deep_expressions_fits_the_worker_stack() {
    let pool = pool(1, 4);
    let nested = format!("{}1 + f(n - 1){}", "(".repeat(10), ")".repeat(10));
    let source = format!(
        "fn f(n) {{ if n <= 0 {{ 0 }} else {{ {nested} }} }} #{{ vars: #{{ depth: f(30) }} }}"
    );
    let output = run(&pool, &source).expect("fits in the stack");
    assert_eq!(vars_of(&output), json!({"depth": 30}));
}

#[test]
fn a_hundred_mebibyte_string_is_stopped_with_a_named_error() {
    let pool = pool(1, 4);
    let error = failure(&pool, "let s = \"x\"; for i in 0..27 { s += s; } s");
    assert_eq!(error.code(), "data_too_large");
    let error = failure(&pool, "let s = \"x\"; s.pad(104857600, 'x'); s");
    assert_eq!(error.code(), "data_too_large");
}

#[test]
fn replacing_into_a_huge_string_is_refused_before_it_allocates() {
    let pool = pool(1, 4);
    let limits = RunLimits {
        memory_bytes: 2 * 1024 * 1024,
        ..RunLimits::default()
    };
    let error = run_with(
        &pool,
        "let s = \"a\"; for i in 0..14 { s += s; } s.replace(\"a\", s); 1",
        limits,
    )
    .expect_err("replace bomb");
    assert_eq!(error.code(), "data_too_large");
}

#[test]
fn a_huge_array_push_is_stopped() {
    let pool = pool(1, 4);
    assert_eq!(
        failure(&pool, "let a = []; loop { a.push(1); }").code(),
        "data_too_large"
    );
    assert_eq!(
        failure(&pool, "let a = []; a.pad(100000000, 0); a").code(),
        "data_too_large"
    );
}

#[test]
fn nesting_containers_cannot_hide_their_size() {
    let pool = pool(1, 4);
    for source in [
        "let a = []; for i in 0..100000 { a = [a]; } 1",
        "let m = #{}; for i in 0..100000 { m = #{ k: m }; } 1",
        "let m = #{ a: 1 }; for i in 0..100000 { m.k = m; } 1",
    ] {
        let error = run_with(&pool, source, roomy()).expect_err(source);
        assert_eq!(error.code(), "data_too_large", "{source}");
    }
}

#[test]
fn map_growth_through_assignment_is_stopped_by_the_memory_budget() {
    let pool = pool(1, 4);
    let limits = RunLimits {
        memory_bytes: 4 * 1024 * 1024,
        max_operations: 5_000_000,
        time: Duration::from_secs(60),
        ..RunLimits::default()
    };
    let error = run_with(
        &pool,
        "let m = #{}; for i in 0..4000000 { m[i.to_string()] = i; } 1",
        limits,
    )
    .expect_err("memory");
    assert_eq!(error, ScriptError::Memory);
}

#[test]
fn catastrophic_backtracking_patterns_finish_in_linear_time() {
    let pool = pool(1, 4);
    let started = Instant::now();
    let output = run_with(
        &pool,
        "let t = \"a\"; for i in 0..13 { t += t; } t += \"!\"; #{ vars: #{ hit: regex_match(\"(a+)+$\", t), n: t.len() } }",
        roomy(),
    )
    .expect("regex finishes");
    assert_eq!(vars_of(&output), json!({"hit": false, "n": 8193}));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn expensive_regex_work_is_refused_before_it_runs_and_is_budgeted_per_run() {
    let pool = pool(1, 4);
    let mut request = default_request();
    request["body"] = json!("a".repeat(4000));
    let with_body = |source: &str| {
        pool.run(job_with(
            compile(source),
            request.clone(),
            RunLimits::default(),
        ))
    };
    let error =
        with_body("regex_match(\"[a-z]{1,1000}[0-9]\", request.body)").expect_err("too costly");
    assert!(error.to_string().contains("regex_too_costly"), "{error}");
    let output = with_body("#{ vars: #{ hit: regex_match(\"^a+$\", request.body) } }")
        .expect("a cheap pattern on the same text");
    assert_eq!(vars_of(&output), json!({"hit": true}));
    let output = with_body(
        "#{ vars: #{ hit: regex_match(\"[a-z]{1,1000}[0-9]\", request.body.sub_string(0, 100)) } }",
    )
    .expect("the same pattern on a short text");
    assert_eq!(vars_of(&output), json!({"hit": false}));
    let error = with_body("let n = 0; loop { regex_match(\"\\\\w+\", request.body); n += 1; }")
        .expect_err("the run budget ends the loop");
    assert_eq!(error, ScriptError::HostBudget);
}

#[test]
fn oversized_regex_programs_are_refused() {
    let pool = pool(1, 4);
    let error = failure(&pool, "regex_match(\"((a{1000}){1000}){1000}\", \"a\")");
    assert_eq!(error.code(), "runtime");
    assert!(error.to_string().contains("invalid_regex"));
    let error = failure(
        &pool,
        "regex_match(\"((((((((((((((((((((((((((((((((((((a))))))))))))))))))))))))))))))))))))\", \"a\")",
    );
    assert!(error.to_string().contains("invalid_regex"));
}

#[test]
fn json_parse_bombs_are_refused() {
    let pool = pool(1, 4);
    let deep = "[".repeat(60_000);
    let mut request = default_request();
    request["body"] = json!(deep);
    let error = pool
        .run(job_with(
            compile("json_parse(request.body)"),
            request,
            RunLimits::default(),
        ))
        .expect_err("too deep");
    assert_eq!(error.code(), "runtime");
    assert!(error.to_string().contains("invalid_json"));

    let wide = format!("[{}0]", "0,".repeat(5000));
    let mut request = default_request();
    request["body"] = json!(wide);
    let error = pool
        .run(job_with(
            compile("json_parse(request.body)"),
            request,
            RunLimits::default(),
        ))
        .expect_err("too wide");
    assert_eq!(error.code(), "data_too_large");

    let mut request = default_request();
    request["body"] = json!("x".repeat(70_000));
    let error = pool
        .run(job_with(
            compile("json_parse(request.body)"),
            request,
            RunLimits::default(),
        ))
        .expect_err("too long");
    assert_eq!(error.code(), "data_too_large");
}

#[test]
fn the_memory_budget_stops_a_script_that_builds_too_much() {
    let pool = pool(1, 4);
    let limits = RunLimits {
        memory_bytes: 1024 * 1024,
        time: Duration::from_secs(20),
        ..RunLimits::default()
    };
    let error = run_with(
        &pool,
        "let base = \"x\"; base.pad(16000, 'y'); let m = #{}; for i in 0..2000 { m[i.to_string()] = base + i.to_string(); } 1",
        limits,
    )
    .expect_err("over budget");
    assert_eq!(error, ScriptError::Memory);
    assert_eq!(error.code(), "memory");
    assert_eq!(error.outcome(), "memory");
}

#[test]
fn an_output_flood_is_truncated_to_the_log_limit() {
    let pool = pool(1, 4);
    let output = run_with(
        &pool,
        "for i in 0..3000 { print(\"line number \" + i); } ()",
        roomy(),
    )
    .expect("flood is bounded");
    assert_eq!(output.log.text.len(), 4096);
    assert!(output.log.truncated);
    assert!(
        output
            .log
            .text
            .starts_with("line number 0\nline number 1\n")
    );
}

#[test]
fn the_returned_values_are_capped() {
    let pool = pool(1, 4);
    let limits = RunLimits {
        max_response_body_bytes: 10,
        ..RunLimits::default()
    };
    let error = run_with(&pool, "#{ response: #{ body: \"01234567890\" } }", limits)
        .expect_err("body too long");
    assert_eq!(error.code(), "invalid_result");

    let limits = RunLimits {
        max_vars_bytes: 100,
        ..RunLimits::default()
    };
    let error = run_with(
        &pool,
        "let big = \"x\"; big.pad(200, 'y'); #{ vars: #{ a: big } }",
        limits,
    )
    .expect_err("vars too large");
    assert_eq!(error.code(), "invalid_result");
}

#[test]
fn too_many_variables_are_stopped() {
    let pool = pool(1, 4);
    let source: String = (0..200)
        .map(|index| format!("let v{index} = {index};\n"))
        .collect();
    let error = failure(&pool, &source);
    assert_eq!(error.code(), "too_many_variables");
}

#[test]
fn eval_and_import_and_dynamic_calls_are_refused_with_named_errors() {
    let compiler = compiler();
    for source in [
        "eval(\"1 + 1\")",
        "import \"module\" as m;",
        "export let x = 1;",
        "let f = Fn(\"print\"); f.call(1)",
        "is_def_fn(\"f\", 1)",
        "is_def_var(\"x\")",
        "let x = 1; curry(x)",
    ] {
        let error = compiler.compile(source).expect_err(source);
        assert_eq!(
            error.diagnostics[0].code,
            ErrorCode::DisabledFeature,
            "{source}"
        );
        assert_eq!(error.diagnostics[0].code.as_str(), "disabled_feature");
    }
}

#[test]
fn custom_syntax_and_modules_do_not_exist() {
    let compiler = compiler();
    assert!(compiler.compile("global::x").is_err());
    assert!(compiler.compile("let m = 1; m::f()").is_err());
}

#[test]
fn expression_depth_is_limited_at_compile_time() {
    let compiler = compiler();
    let nested = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    let error = compiler.compile(&nested).expect_err("too deep");
    assert_eq!(error.diagnostics[0].code, ErrorCode::ExpressionTooDeep);
    let allowed = format!("{}1{}", "(".repeat(30), ")".repeat(30));
    assert!(compiler.compile(&allowed).is_ok());
}

#[test]
fn the_function_count_is_limited_at_compile_time() {
    let compiler = compiler();
    let source: String = (0..40)
        .map(|index| format!("fn f{index}() {{ {index} }}\n"))
        .collect();
    let error = compiler.compile(&source).expect_err("too many functions");
    assert_eq!(error.diagnostics[0].code, ErrorCode::TooManyFunctions);
}

#[test]
fn the_source_size_is_limited() {
    let compiler = compiler();
    let exact = format!("{}\n()", "/".repeat(16 * 1024 - 3));
    assert_eq!(exact.len(), 16 * 1024);
    assert!(compiler.compile(&exact).is_ok());
    let error = compiler
        .compile(&format!("{exact} "))
        .expect_err("one byte over");
    assert_eq!(error.diagnostics[0].code, ErrorCode::SourceTooLarge);
    assert_eq!(error.diagnostics.len(), 1);
}

#[test]
fn the_operation_limit_depends_on_the_phase() {
    let pool = pool(1, 4);
    let source = "let n = 0; while n < 100000 { n += 1; } #{ vars: #{ n: n } }";
    let before = RunLimits {
        time: Duration::from_secs(30),
        ..RunLimits::for_phase(ScriptPhase::BeforeResponse, Duration::from_secs(30))
    };
    assert_eq!(
        run_with(&pool, source, before).expect_err("before response"),
        ScriptError::Operations
    );
    let after = RunLimits::for_phase(ScriptPhase::AfterResponse, Duration::from_secs(30));
    let output = run_with(&pool, source, after).expect("after response");
    assert_eq!(vars_of(&output), json!({"n": 100000}));
}

#[test]
fn zero_limits_that_rhai_reads_as_unlimited_are_rejected() {
    let pool = pool(1, 4);
    let zero_operations = RunLimits {
        max_operations: 0,
        ..RunLimits::default()
    };
    assert_eq!(
        run_with(&pool, "()", zero_operations).expect_err("zero operations"),
        ScriptError::Limits(LimitsError::Zero("max_operations"))
    );
    let zero_time = RunLimits {
        time: Duration::ZERO,
        ..RunLimits::default()
    };
    assert_eq!(
        run_with(&pool, "()", zero_time).expect_err("zero time"),
        ScriptError::Limits(LimitsError::Zero("time"))
    );
    for broken in [
        EngineLimits {
            max_call_levels: 0,
            ..EngineLimits::default()
        },
        EngineLimits {
            max_string_bytes: 0,
            ..EngineLimits::default()
        },
        EngineLimits {
            max_array_size: 0,
            ..EngineLimits::default()
        },
        EngineLimits {
            max_variables: 0,
            ..EngineLimits::default()
        },
    ] {
        assert!(matches!(
            ScriptCompiler::new(broken),
            Err(LimitsError::Zero(_))
        ));
    }
    assert!(matches!(
        ScriptCompiler::new(EngineLimits {
            max_call_levels: 1000,
            ..EngineLimits::default()
        }),
        Err(LimitsError::AboveCeiling { .. })
    ));
}

#[test]
fn the_deadline_with_a_fake_clock_stops_the_script_promptly() {
    let pool = pool(1, 4);
    let clock = Arc::new(SteppingClock::new(Duration::from_millis(1)));
    let mut spin = job_with(
        compile("loop {}"),
        default_request(),
        RunLimits {
            max_operations: 1_000_000_000,
            time: Duration::from_millis(50),
            ..RunLimits::default()
        },
    );
    spin.clock = clock.clone();
    let error = pool.run(spin).expect_err("deadline");
    assert_eq!(error, ScriptError::Time);
    assert_eq!(error.outcome(), "time");
    assert!(clock.calls() >= 50);
    assert!(clock.calls() < 70);
}

#[test]
fn a_frozen_clock_never_fires_the_deadline_but_the_operation_limit_still_does() {
    let pool = pool(1, 4);
    let mut spin = job(compile("while true {}"));
    spin.clock = Arc::new(FrozenClock);
    assert_eq!(
        pool.run(spin).expect_err("operations"),
        ScriptError::Operations
    );
}

#[test]
fn queue_wait_counts_against_the_deadline() {
    let pool = pool(1, 4);
    let mut late = job(compile("#{ vars: #{ ran: true } }"));
    late.clock = Arc::new(SteppingClock::new(Duration::from_secs(1)));
    late.limits.time = Duration::from_millis(100);
    assert_eq!(
        pool.run(late).expect_err("expired in the queue"),
        ScriptError::Time
    );
}

#[test]
fn the_deadline_with_the_real_clock_stops_the_script_within_a_small_slack() {
    let pool = pool(1, 4);
    let limits = RunLimits {
        max_operations: 10_000_000_000,
        time: Duration::from_millis(100),
        ..RunLimits::default()
    };
    let started = Instant::now();
    let error = run_with(&pool, "let n = 0; loop { n += 1; }", limits).expect_err("deadline");
    let elapsed = started.elapsed();
    assert_eq!(error, ScriptError::Time);
    assert!(elapsed >= Duration::from_millis(100), "{elapsed:?}");
    assert!(elapsed < Duration::from_millis(600), "{elapsed:?}");
}

#[test]
fn long_host_calls_are_bounded_by_the_host_budget() {
    let pool = pool(1, 4);
    let limits = RunLimits {
        max_operations: 10_000_000_000,
        time: Duration::from_secs(30),
        ..RunLimits::default()
    };
    let started = Instant::now();
    let error = run_with(
        &pool,
        "let t = \"x\"; t.pad(16000, 'y'); loop { sha256(t); }",
        limits,
    )
    .expect_err("host budget");
    assert_eq!(error, ScriptError::HostBudget);
    assert_eq!(error.outcome(), "operations");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn compiling_on_the_pool_applies_the_same_limits() {
    let pool = pool(1, 4);
    let compiler = compiler();
    assert!(pool.compile(&compiler, "#{ }").is_ok());
    let error = pool.compile(&compiler, "let x = ;").expect_err("syntax");
    assert!(matches!(error, ScriptError::Compile(_)));
    let nested = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    assert!(matches!(
        pool.compile(&compiler, &nested),
        Err(ScriptError::Compile(_))
    ));
    let oversized = "a".repeat(17 * 1024);
    assert!(matches!(
        pool.compile(&compiler, &oversized),
        Err(ScriptError::Compile(error)) if error.diagnostics[0].code == ErrorCode::SourceTooLarge
    ));
}
