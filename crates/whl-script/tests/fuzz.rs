mod common;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use common::*;
use proptest::prelude::*;
use whl_script::{RunLimits, ScriptError, ScriptPool};

const DEADLINE: Duration = Duration::from_millis(50);
const SLACK: Duration = Duration::from_millis(1500);

fn shared_pool() -> &'static ScriptPool {
    static POOL: OnceLock<ScriptPool> = OnceLock::new();
    POOL.get_or_init(|| pool(2, 64))
}

fn expression() -> impl Strategy<Value = String> {
    let leaf = prop_oneof![
        (0i64..1000).prop_map(|number| number.to_string()),
        Just("\"abc\"".to_owned()),
        Just("true".to_owned()),
        Just("()".to_owned()),
        Just("x".to_owned()),
        Just("request.method".to_owned()),
        Just("request.headers".to_owned()),
        Just("vars.count".to_owned()),
        Just("[1, 2, 3]".to_owned()),
        Just("#{ a: 1, b: \"two\" }".to_owned()),
        Just("uuid()".to_owned()),
        Just("now()".to_owned()),
        Just("random_int(1, 6)".to_owned()),
    ];
    leaf.prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a} + {b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a} * {b})")),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("({a} == {b})")),
            inner
                .clone()
                .prop_map(|a| format!("sha256(({a}).to_string())")),
            inner.clone().prop_map(|a| format!("json_stringify({a})")),
            inner
                .clone()
                .prop_map(|a| format!("json_parse(({a}).to_string())")),
            inner
                .clone()
                .prop_map(|a| format!("base64_encode(({a}).to_string())")),
            inner
                .clone()
                .prop_map(|a| format!("base64_decode(({a}).to_string())")),
            inner
                .clone()
                .prop_map(|a| format!("hex(({a}).to_string())")),
            inner
                .clone()
                .prop_map(|a| format!("regex_match(\"a+b*\", ({a}).to_string())")),
            inner
                .clone()
                .prop_map(|a| format!("regex_captures(\"(\\\\w+)\", ({a}).to_string())")),
            inner.clone().prop_map(|a| format!("[{a}, {a}, {a}]")),
            inner.clone().prop_map(|a| format!("#{{ k: {a} }}")),
            inner.clone().prop_map(|a| format!("({a}).len()")),
            inner.prop_map(|a| format!("hmac_sha256(\"key\", ({a}).to_string())")),
        ]
    })
}

fn statement() -> impl Strategy<Value = String> {
    let simple = prop_oneof![
        expression().prop_map(|e| format!("x = {e};")),
        expression().prop_map(|e| format!("x += {e};")),
        expression().prop_map(|e| format!("vars.k = {e};")),
        expression().prop_map(|e| format!("print({e});")),
        expression().prop_map(|e| format!("let y = {e};")),
        Just("x = [x, x];".to_owned()),
        Just("x = #{ k: x };".to_owned()),
        Just("x = x + x;".to_owned()),
        Just("x.pad(100000, 'z');".to_owned()),
    ];
    simple.prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            inner
                .clone()
                .prop_map(|body| format!("while true {{ {body} }}")),
            inner.clone().prop_map(|body| format!("loop {{ {body} }}")),
            (1u32..50, inner.clone())
                .prop_map(|(count, body)| format!("for i in 0..{count} {{ {body} }}")),
            (expression(), inner.clone(), inner.clone())
                .prop_map(|(c, a, b)| format!("if {c} {{ {a} }} else {{ {b} }}")),
            inner
                .clone()
                .prop_map(|body| format!("try {{ {body} }} catch {{ x = 0; }}")),
            (0u32..80, inner.clone()).prop_map(|(depth, body)| {
                format!("fn rec(n) {{ if n > 0 {{ rec(n - 1) }} else {{ {body} }} }} rec({depth});")
            }),
            (inner.clone(), inner).prop_map(|(a, b)| format!("{a} {b}")),
        ]
    })
}

fn program() -> impl Strategy<Value = String> {
    prop::collection::vec(statement(), 1..5).prop_map(|statements| {
        format!(
            "let x = 1;\n{}\n#{{ vars: #{{ done: true }} }}",
            statements.join("\n")
        )
    })
}

fn limits() -> RunLimits {
    RunLimits {
        max_operations: 200_000,
        time: DEADLINE,
        memory_bytes: 8 * 1024 * 1024,
        ..roomy()
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn random_scripts_never_panic_the_host_and_always_terminate_within_the_deadline(source in program()) {
        match compiler().compile(&source) {
            Err(error) => {
                prop_assert!(!error.diagnostics.is_empty());
                prop_assert_eq!(error.diagnostics.len(), 1);
                for diagnostic in &error.diagnostics {
                    prop_assert!(diagnostic.position.line >= 1);
                    prop_assert!(diagnostic.position.column >= 1);
                    prop_assert!(diagnostic.position.offset <= source.len());
                }
            }
            Ok(script) => {
                let started = Instant::now();
                let result = shared_pool().run(job_with(script, default_request(), limits()));
                let elapsed = started.elapsed();
                prop_assert!(elapsed < DEADLINE + SLACK, "{elapsed:?} for {source}");
                match result {
                    Ok(output) => prop_assert!(output.log.text.len() <= 4096),
                    Err(error) => prop_assert!(
                        !matches!(
                            error,
                            ScriptError::Panic
                                | ScriptError::ResultTimeout
                                | ScriptError::ShutDown
                                | ScriptError::Overloaded
                                | ScriptError::InvalidInput(_)
                                | ScriptError::Limits(_)
                        ),
                        "{error:?} for {source}"
                    ),
                }
            }
        }
    }
}
