mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use time::macros::datetime;
use whl_script::{
    CacheConfig, ErrorCode, RandomSeed, RunLimits, ScriptCache, ScriptCompiler, ScriptError,
    ScriptHash, ScriptPool, SeededRandom,
};

fn output_of(pool: &ScriptPool, source: &str) -> whl_script::ScriptOutput {
    run(pool, source).expect("run succeeds")
}

fn seeded(job: &mut whl_script::ScriptJob, label: &[u8]) {
    job.random = Box::new(SeededRandom::from_seed(&RandomSeed::from_bytes(label)));
}

#[test]
fn the_same_inputs_give_the_same_result() {
    let source = "print(\"log\"); #{ response: #{ status: 200 + random_int(0, 50), body: uuid() + now() }, vars: #{ r: random_int(1, 1000000), h: sha256(request.body) } }";
    let script = compile(source);
    let mut outputs = Vec::new();
    for round in 0..4 {
        let pool = pool(1 + round % 2, 4);
        let mut input = job(script.clone());
        input.now = datetime!(2026-01-02 03:04:05 UTC);
        outputs.push(pool.run(input).expect("run"));
    }
    for other in &outputs[1..] {
        assert_eq!(other.response, outputs[0].response);
        assert_eq!(other.vars, outputs[0].vars);
        assert_eq!(other.log, outputs[0].log);
        assert_eq!(other.stats.operations, outputs[0].stats.operations);
    }
}

#[test]
fn different_seeds_or_inputs_give_different_results() {
    let pool = pool(1, 4);
    let script = compile("#{ vars: #{ u: uuid(), m: request.method } }");
    let mut first = job(script.clone());
    seeded(&mut first, b"one");
    let mut second = job(script.clone());
    seeded(&mut second, b"two");
    let mut third = job_with(script, json!({"method": "PUT"}), RunLimits::default());
    seeded(&mut third, b"one");
    let first = pool.run(first).expect("run");
    let second = pool.run(second).expect("run");
    let third = pool.run(third).expect("run");
    assert_ne!(first.vars["u"], second.vars["u"]);
    assert_eq!(first.vars["u"], third.vars["u"]);
    assert_ne!(first.vars["m"], third.vars["m"]);
}

#[test]
fn no_value_leaks_between_runs_on_the_same_worker() {
    let pool = pool(1, 8);
    let compiler = compiler();

    output_of(
        &pool,
        "let leaked_variable = \"secret-A\"; const LEAKED_CONSTANT = 7; ()",
    );
    for source in ["leaked_variable", "LEAKED_CONSTANT"] {
        let error = compiler.compile(source).expect_err(source);
        assert_eq!(error.diagnostics[0].code, ErrorCode::UndefinedVariable);
    }

    let defined = output_of(
        &pool,
        "fn leaked_function() { 7 } #{ vars: #{ v: leaked_function() } }",
    );
    assert_eq!(vars_of(&defined), json!({"v": 7}));
    let error = run(&pool, "leaked_function()").expect_err("function must not survive");
    assert_eq!(error.code(), "undefined_function");

    let shadowed = output_of(
        &pool,
        "fn sha256(text) { \"fake\" } #{ vars: #{ h: sha256(\"abc\") } }",
    );
    assert_eq!(vars_of(&shadowed), json!({"h": "fake"}));
    let real = output_of(&pool, "#{ vars: #{ h: sha256(\"abc\") } }");
    assert_eq!(
        vars_of(&real),
        json!({"h": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"})
    );

    let mut mutating = job(compile(
        "vars.counter = 1; vars.secret = request.marker; ()",
    ));
    mutating.request = json!({"marker": "run-A"});
    let mutated = pool.run(mutating).expect("run");
    assert_eq!(vars_of(&mutated), json!({"counter": 1, "secret": "run-A"}));
    let mut observing = job(compile(
        "#{ vars: #{ counter: vars.counter, secret: vars.secret, marker: request.marker } }",
    ));
    observing.request = json!({"marker": "run-B"});
    let observed = pool.run(observing).expect("run");
    assert_eq!(
        vars_of(&observed),
        json!({"counter": null, "secret": null, "marker": "run-B"})
    );

    let noisy = output_of(&pool, "print(\"secret-log-A\"); ()");
    assert_eq!(noisy.log.text, "secret-log-A\n");
    let quiet = output_of(&pool, "()");
    assert_eq!(quiet.log.text, "");

    let draw = "#{ vars: #{ u: uuid(), r: random_int(1, 1000000000) } }";
    let first_draw = output_of(&pool, draw);
    let second_draw = output_of(&pool, draw);
    assert_eq!(first_draw.vars, second_draw.vars);

    let counted = "let n = 0; for i in 0..10 { n += i; } n; ()";
    let first_count = output_of(&pool, counted).stats.operations;
    let second_count = output_of(&pool, counted).stats.operations;
    assert_eq!(first_count, second_count);

    let tamper = run(&pool, "request.method = \"GET\"; ()").expect_err("read-only");
    assert_eq!(tamper.code(), "runtime");
    let pristine = output_of(&pool, "#{ vars: #{ m: request.method } }");
    assert_eq!(vars_of(&pristine), json!({"m": "POST"}));

    let mut early = job(compile("#{ vars: #{ n: now() } }"));
    early.now = datetime!(2020-01-01 00:00:00 UTC);
    let mut late = job(compile("#{ vars: #{ n: now() } }"));
    late.now = datetime!(2030-01-01 00:00:00 UTC);
    assert_ne!(
        pool.run(early).expect("run").vars,
        pool.run(late).expect("run").vars
    );
}

#[test]
fn an_aborted_run_leaves_no_residue_for_the_next_one_on_the_same_worker() {
    let pool = pool(1, 8);
    let tight = RunLimits {
        memory_bytes: 512 * 1024,
        time: Duration::from_secs(30),
        ..RunLimits::default()
    };
    let error = run_with(
        &pool,
        "let base = \"x\"; base.pad(16000, 'y'); let m = #{}; for i in 0..2000 { m[i.to_string()] = base + i.to_string(); } 1",
        tight,
    )
    .expect_err("memory");
    assert_eq!(error, ScriptError::Memory);
    assert!(!whl_script::memory_exceeded());

    let spin = RunLimits {
        max_operations: u64::MAX / 2,
        time: Duration::from_millis(30),
        ..RunLimits::default()
    };
    assert_eq!(
        run_with(&pool, "loop {}", spin).expect_err("time"),
        ScriptError::Time
    );

    let mut cancelled = job(compile("loop {}"));
    cancelled.limits.max_operations = u64::MAX / 2;
    cancelled.limits.time = Duration::from_secs(60);
    let pending = pool.submit(cancelled).expect("submitted");
    std::thread::sleep(Duration::from_millis(50));
    pending.cancel();
    assert_eq!(
        pending.wait().expect_err("cancelled"),
        ScriptError::Cancelled
    );

    let healthy = output_of(&pool, "#{ vars: #{ ok: true } }");
    assert_eq!(vars_of(&healthy), json!({"ok": true}));
    assert!(healthy.stats.peak_memory_bytes < 100_000);
}

#[test]
fn one_compiled_script_shared_across_threads_never_mixes_inputs() {
    let pool = Arc::new(pool(2, 64));
    let script = compile("#{ vars: #{ marker: request.marker, echo: sha256(request.marker) } }");
    let handles: Vec<_> = (0..4)
        .map(|worker| {
            let pool = Arc::clone(&pool);
            let script = script.clone();
            std::thread::spawn(move || {
                for round in 0..25 {
                    let marker = format!("thread-{worker}-round-{round}");
                    let input = job_with(
                        script.clone(),
                        json!({ "marker": marker }),
                        RunLimits::default(),
                    );
                    let output = pool.run(input).expect("run");
                    assert_eq!(output.vars["marker"], json!(marker));
                    assert_eq!(output.vars.len(), 2);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().expect("thread finishes");
    }
}

#[test]
fn nothing_that_smells_like_io_exists() {
    let pool = pool(1, 8);
    let names = [
        "open",
        "read_file",
        "write_file",
        "file_read",
        "readFile",
        "http_get",
        "http_post",
        "fetch",
        "connect",
        "socket",
        "exec",
        "system",
        "spawn",
        "shell",
        "env",
        "getenv",
        "args",
        "abort",
        "timestamp",
        "sleep",
        "now_ms",
        "load",
        "require",
        "include",
        "dlopen",
        "std_fs_read",
        "stdin",
        "read_line",
        "input",
        "print_stderr",
    ];
    for name in names {
        for call in [format!("{name}()"), format!("{name}(\"/etc/passwd\")")] {
            match compiler().compile(&call) {
                Err(error) => {
                    assert_eq!(
                        error.diagnostics[0].code,
                        ErrorCode::DisabledFeature,
                        "{call}"
                    )
                }
                Ok(script) => {
                    let error = pool.run(job(script)).expect_err(&call);
                    assert_eq!(error.code(), "undefined_function", "{call}: {error}");
                }
            }
        }
    }
    for source in [
        "import \"fs\" as fs; fs::read(\"/etc/passwd\")",
        "eval(\"open('/etc/passwd')\")",
        "std::fs::read(\"/etc/passwd\")",
        "global::open(\"x\")",
    ] {
        assert!(compiler().compile(source).is_err(), "{source}");
    }
}

#[test]
fn diagnostics_carry_line_column_and_offset() {
    let compiler = compiler();
    let source = "let a = 1;\nlet b = ;\nlet c = 3;";
    let error = compiler.compile(source).expect_err("syntax error");
    let diagnostic = &error.diagnostics[0];
    assert_eq!(diagnostic.code, ErrorCode::Syntax);
    assert_eq!(diagnostic.position.line, 2);
    assert_eq!(diagnostic.position.column, 9);
    assert_eq!(
        diagnostic.position.offset,
        source.find("= ;").expect("error site") + 2
    );
    assert_eq!(diagnostic.position.length, 1);
    assert!(!error.truncated);
    assert_eq!(error.diagnostics.len(), 1);
    assert_eq!(diagnostic.code.field_code(), "script_invalid");
}

#[test]
fn diagnostic_columns_count_characters_not_bytes() {
    let source = "let s = \"éééé\"; let x = ;";
    let error = compiler().compile(source).expect_err("syntax error");
    let position = error.diagnostics[0].position;
    let expected_column = source
        .chars()
        .rev()
        .position(|c| c == ';')
        .map_or(0, |from_end| source.chars().count() - from_end);
    assert_eq!(position.column as usize, expected_column);
    assert_eq!(position.offset, source.rfind(';').expect("semicolon"));
    assert!(position.offset > position.column as usize);
    assert_eq!(position.line, 1);
}

#[test]
fn diagnostics_cover_incomplete_and_undefined_input() {
    let compiler = compiler();
    let error = compiler.compile("let x = ").expect_err("incomplete");
    assert_eq!(error.diagnostics[0].code, ErrorCode::UnexpectedEnd);
    assert_eq!(error.diagnostics[0].position.line, 1);
    let error = compiler
        .compile("let a = 1;\n\n  missing + 1")
        .expect_err("undefined");
    assert_eq!(error.diagnostics[0].code, ErrorCode::UndefinedVariable);
    assert_eq!(error.diagnostics[0].position.line, 3);
    assert_eq!(error.diagnostics[0].position.column, 3);
    let error = compiler
        .compile("const A = 1; A = 2;")
        .expect_err("constant");
    assert_eq!(error.diagnostics[0].code, ErrorCode::AssignmentToConstant);
    let error = compiler
        .compile("let s = \"open")
        .expect_err("unterminated");
    assert!(error.diagnostics.len() <= whl_script::MAX_DIAGNOSTICS);
}

#[test]
fn runtime_errors_carry_the_line_and_column() {
    let pool = pool(1, 4);
    let error = run(&pool, "let a = 1;\nlet b = a / 0;\nb").expect_err("division");
    match error {
        ScriptError::Runtime {
            position: Some(position),
            ..
        } => {
            assert_eq!(position.line, 2);
            assert!(position.column >= 9);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn the_cache_returns_compiled_scripts_by_hash_and_bounds_its_size() {
    let pool = pool(1, 4);
    let compiler = compiler();
    let cache = ScriptCache::new(CacheConfig {
        max_entries: 2,
        max_weight_bytes: 1 << 20,
    })
    .expect("cache");

    let first = cache
        .get_or_compile(&pool, &compiler, "1")
        .expect("compile");
    assert_eq!(first.hash(), ScriptHash::of("1"));
    assert_eq!(first.hash().to_hex().len(), 64);
    let again = cache.get_or_compile(&pool, &compiler, "1").expect("hit");
    assert_eq!(again.hash(), first.hash());
    assert_eq!(cache.stats().hits, 1);

    cache
        .get_or_compile(&pool, &compiler, "2")
        .expect("compile");
    cache
        .get_or_compile(&pool, &compiler, "1")
        .expect("touch the oldest");
    cache
        .get_or_compile(&pool, &compiler, "3")
        .expect("compile");
    let stats = cache.stats();
    assert_eq!(stats.entries, 2);
    assert_eq!(stats.evictions, 1);
    assert!(cache.get(&ScriptHash::of("1")).is_some());
    assert!(cache.get(&ScriptHash::of("2")).is_none());
    assert!(cache.get(&ScriptHash::of("3")).is_some());
}

#[test]
fn the_cache_bounds_total_weight_skips_failures_and_respects_limit_changes() {
    let pool = pool(1, 4);
    let compiler = compiler();
    let cache = ScriptCache::new(CacheConfig {
        max_entries: 100,
        max_weight_bytes: 1500,
    })
    .expect("cache");
    cache
        .get_or_compile(&pool, &compiler, "1")
        .expect("compile");
    cache
        .get_or_compile(&pool, &compiler, "2")
        .expect("compile");
    assert_eq!(cache.stats().entries, 1);
    assert!(cache.stats().weight_bytes <= 1500);

    let large = format!("let long_name = 1; {}", " ".repeat(100));
    cache
        .get_or_compile(&pool, &compiler, &large)
        .expect("compile");
    assert!(cache.get(&ScriptHash::of(&large)).is_none());

    let before = cache.stats().entries;
    assert!(cache.get_or_compile(&pool, &compiler, "let x = ;").is_err());
    assert_eq!(cache.stats().entries, before);

    let roomy_cache = ScriptCache::new(CacheConfig::default()).expect("cache");
    let strict = ScriptCompiler::new(whl_script::EngineLimits {
        max_expression_depth: 8,
        ..whl_script::EngineLimits::default()
    })
    .expect("limits");
    let source = format!("{}1{}", "(".repeat(20), ")".repeat(20));
    roomy_cache
        .get_or_compile(&pool, &compiler, &source)
        .expect("allowed by the default limits");
    let refused = roomy_cache.get_or_compile(&pool, &strict, &source);
    assert!(matches!(refused, Err(ScriptError::Compile(_))));

    assert!(
        ScriptCache::new(CacheConfig {
            max_entries: 0,
            max_weight_bytes: 1,
        })
        .is_err()
    );
}

#[test]
fn debug_output_of_compiled_scripts_never_includes_the_source() {
    let script = compile("let marker_secret = \"hunter2\"; ()");
    assert!(!format!("{script:?}").contains("hunter2"));
    let value: Value = json!(script.source_bytes());
    assert!(value.as_u64().is_some());
}
