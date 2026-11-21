use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rhai::Engine;
use time::OffsetDateTime;

use crate::engine::build_engine;
use crate::env::SystemClock;
use crate::guard::RunGuard;
use crate::host::{HostLimits, HostState, register};
use crate::limits::EngineLimits;
use crate::{
    PoolConfig, RandomSeed, RunLimits, ScriptCompiler, ScriptError, ScriptJob, ScriptPool,
    SeededRandom, TenantKey,
};

const ALLOWED_FUNCTIONS: &[&str] = &[
    "!",
    "!=",
    "%",
    "&",
    "*",
    "**",
    "+",
    "+=",
    "-",
    "/",
    "<",
    "<<",
    "<=",
    "==",
    ">",
    ">=",
    ">>",
    "E",
    "PI",
    "^",
    "abs",
    "acos",
    "acosh",
    "all",
    "append",
    "asin",
    "asinh",
    "atan",
    "atanh",
    "base64_decode",
    "base64_encode",
    "bits",
    "bytes",
    "ceiling",
    "chars",
    "chop",
    "clear",
    "contains",
    "cos",
    "cosh",
    "crop",
    "debug",
    "dedup",
    "drain",
    "end",
    "ends_with",
    "exp",
    "extract",
    "fill_with",
    "filter",
    "find",
    "find_map",
    "floor",
    "for_each",
    "fraction",
    "get",
    "get$bits",
    "get$bytes",
    "get$ceiling",
    "get$chars",
    "get$end",
    "get$floor",
    "get$fraction",
    "get$int",
    "get$is_anonymous",
    "get$is_empty",
    "get$is_even",
    "get$is_exclusive",
    "get$is_finite",
    "get$is_inclusive",
    "get$is_infinite",
    "get$is_nan",
    "get$is_odd",
    "get$is_zero",
    "get$len",
    "get$name",
    "get$round",
    "get$start",
    "hex",
    "hmac_sha256",
    "hypot",
    "index_of",
    "insert",
    "int",
    "is_anonymous",
    "is_empty",
    "is_even",
    "is_exclusive",
    "is_finite",
    "is_inclusive",
    "is_infinite",
    "is_nan",
    "is_odd",
    "is_zero",
    "json_parse",
    "json_stringify",
    "keys",
    "len",
    "ln",
    "log",
    "make_lower",
    "make_upper",
    "map",
    "max",
    "min",
    "mixin",
    "name",
    "now",
    "order",
    "order_by",
    "order_desc",
    "pad",
    "parse_float",
    "parse_int",
    "pop",
    "print",
    "push",
    "random_int",
    "range",
    "reduce",
    "reduce_rev",
    "regex_captures",
    "regex_match",
    "remove",
    "replace",
    "retain",
    "reverse",
    "round",
    "set",
    "sha256",
    "shift",
    "sign",
    "sin",
    "sinh",
    "some",
    "sort",
    "sort_by",
    "sort_desc",
    "splice",
    "split",
    "split_rev",
    "sqrt",
    "start",
    "starts_with",
    "sub_string",
    "tan",
    "tanh",
    "to_binary",
    "to_blob",
    "to_chars",
    "to_debug",
    "to_degrees",
    "to_float",
    "to_hex",
    "to_int",
    "to_json",
    "to_lower",
    "to_octal",
    "to_radians",
    "to_string",
    "to_upper",
    "trim",
    "truncate",
    "uuid",
    "values",
    "zip",
    "|",
];

const FORBIDDEN_FUNCTIONS: &[&str] = &[
    "sleep",
    "exit",
    "take",
    "tag",
    "set_tag",
    "get$tag",
    "set$tag",
    "parse_json",
    "get_fn_metadata_list",
    "eval",
    "timestamp",
    "elapsed",
];

const MAX_SWEEP_CALLS: usize = 9000;
const COMBOS_PER_SIGNATURE: usize = 36;
const INFIX: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "<<", ">>", "&", "|", "^", "==", "!=", "<", "<=", ">", ">=",
    "+=",
];

fn full_engine() -> Engine {
    let limits = EngineLimits::default();
    let mut engine = build_engine(&limits);
    let guard = Arc::new(RunGuard::new(
        Arc::new(SystemClock::new()),
        Duration::from_secs(60),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        1 << 20,
    ));
    let state = Arc::new(HostState::new(
        guard,
        "2026-01-01T00:00:00Z".to_owned(),
        HostLimits {
            max_string_bytes: limits.max_string_bytes,
            max_array_size: limits.max_array_size,
            max_map_size: limits.max_map_size,
            max_map_key_bytes: limits.max_map_key_bytes,
            max_text_bytes: 64 * 1024,
            regex_size_limit_bytes: 256 * 1024,
        },
    ));
    register(&mut engine, &state);
    engine
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Signature {
    name: String,
    parameters: Vec<String>,
}

fn parse_signature(text: &str) -> Option<Signature> {
    let open = text.find('(')?;
    let close = text.find(')')?;
    let name = text.get(..open)?.to_owned();
    let inner = text.get(open + 1..close)?;
    let parameters = inner
        .split(", ")
        .filter(|part| !part.is_empty())
        .map(|part| {
            let kind = part.split_once(": ").map_or(part, |(_, kind)| kind);
            kind.trim_start_matches("&mut ")
                .trim_start_matches('&')
                .to_owned()
        })
        .collect();
    Some(Signature { name, parameters })
}

fn signatures(engine: &Engine) -> Vec<Signature> {
    engine
        .gen_fn_signatures(true)
        .iter()
        .filter_map(|text| parse_signature(text))
        .collect()
}

#[test]
fn the_registered_function_surface_matches_the_allow_list_exactly() {
    let engine = full_engine();
    let registered: BTreeSet<String> = signatures(&engine)
        .into_iter()
        .map(|signature| signature.name)
        .collect();
    let allowed: BTreeSet<String> = ALLOWED_FUNCTIONS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let unexpected: Vec<&String> = registered.difference(&allowed).collect();
    let missing: Vec<&String> = allowed.difference(&registered).collect();
    assert!(
        unexpected.is_empty(),
        "unexpected functions: {unexpected:?}"
    );
    assert!(missing.is_empty(), "missing functions: {missing:?}");
    for name in FORBIDDEN_FUNCTIONS {
        assert!(!registered.contains(*name), "{name} must not be registered");
    }
}

#[test]
fn every_signature_text_is_parsed_by_the_sweep() {
    let engine = full_engine();
    let raw = engine.gen_fn_signatures(true);
    assert_eq!(raw.len(), signatures(&engine).len());
}

fn arguments_for(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "i64" => Some(&[
            "0",
            "1",
            "-1",
            "9223372036854775807",
            "(-9223372036854775807 - 1)",
            "100000",
        ]),
        "f64" => Some(&["0.0", "-1.5", "1.0e308", "(0.0 / 0.0)", "(1.0e308 * 10.0)"]),
        "string" => Some(&["\"\"", "\"a\"", "\"abc\"", "\"h\u{e9}llo\"", "big"]),
        "char" => Some(&["'a'", "'\\0'", "'\u{e9}'"]),
        "bool" => Some(&["true", "false"]),
        "array" => Some(&["[]", "[1]", "[3, 1, 2]", "bigarr"]),
        "map" => Some(&["#{}", "#{ a: 1 }", "bigmap"]),
        "Dynamic" | "_" | "types::dynamic::Dynamic" => Some(&["()", "0", "\"\"", "[]"]),
        "Fn" => Some(&["|x| x", "|a, b| a"]),
        "range" => Some(&["(0..5)", "(0..9223372036854775807)", "(5..0)"]),
        "range=" => Some(&["(0..=5)", "(0..=9223372036854775807)", "(5..=0)"]),
        "()" => Some(&["()"]),
        _ => None,
    }
}

fn combinations(pools: &[&'static [&'static str]]) -> Vec<Vec<&'static str>> {
    let total: usize = pools.iter().map(|pool| pool.len()).product();
    if total == 0 {
        return Vec::new();
    }
    let picks: Vec<usize> = if total <= COMBOS_PER_SIGNATURE {
        (0..total).collect()
    } else {
        (0..COMBOS_PER_SIGNATURE)
            .map(|index| (index * 7919 + index * index) % total)
            .collect::<BTreeSet<usize>>()
            .into_iter()
            .collect()
    };
    picks
        .into_iter()
        .map(|mut pick| {
            pools
                .iter()
                .map(|pool| {
                    let item = pool[pick % pool.len()];
                    pick /= pool.len();
                    item
                })
                .collect()
        })
        .collect()
}

fn call_source(name: &str, arguments: &[&str]) -> String {
    let mut source = String::new();
    source.push_str("let big = \"x\"; big.pad(16000, 'y');\n");
    source.push_str("let bigarr = []; bigarr.pad(1000, 7);\n");
    source.push_str("let bigmap = #{}; for i in 0..100 { bigmap[i.to_string()] = i; }\n");
    for (index, argument) in arguments.iter().enumerate() {
        source.push_str(&format!("let a{index} = {argument};\n"));
    }
    let names: Vec<String> = (0..arguments.len())
        .map(|index| format!("a{index}"))
        .collect();
    let call = if let Some(property) = name.strip_prefix("get$") {
        format!("a0.{property}")
    } else if INFIX.contains(&name) && arguments.len() == 2 {
        format!("a0 {name} a1")
    } else if (name == "!" || name == "-" || name == "+") && arguments.len() == 1 {
        format!("{name}a0")
    } else if arguments.is_empty() {
        format!("{name}()")
    } else {
        format!("a0.{name}({})", names[1..].join(", "))
    };
    if name == "+=" {
        source.push_str(&format!("{call};\n()"));
    } else {
        source.push_str(&format!("let result = {call};\n()"));
    }
    source
}

fn operator_signatures() -> Vec<Signature> {
    let mut found = Vec::new();
    for name in INFIX {
        for kind in ["i64", "f64", "bool"] {
            found.push(Signature {
                name: (*name).to_owned(),
                parameters: vec![kind.to_owned(), kind.to_owned()],
            });
        }
        found.push(Signature {
            name: (*name).to_owned(),
            parameters: vec!["i64".to_owned(), "f64".to_owned()],
        });
    }
    found
}

fn sweep_job(source: &str, limits: RunLimits) -> Result<ScriptJob, ScriptError> {
    let compiler = ScriptCompiler::new(EngineLimits::default())?;
    let script = compiler.compile(source)?;
    Ok(ScriptJob {
        tenant: TenantKey::new("sweep"),
        script,
        request: serde_json::json!({}),
        vars: serde_json::Map::new(),
        now: OffsetDateTime::UNIX_EPOCH,
        limits,
        clock: Arc::new(SystemClock::new()),
        random: Box::new(SeededRandom::from_seed(&RandomSeed::from_bytes(b"sweep"))),
    })
}

#[test]
fn degenerate_arguments_never_hang_or_outrun_the_deadline_on_any_registered_function() {
    let engine = full_engine();
    let pool = ScriptPool::new(PoolConfig {
        result_grace: Duration::from_secs(4),
        stuck_grace: Duration::from_secs(4),
        shutdown_timeout: Duration::from_millis(500),
        ..PoolConfig::default()
    })
    .expect("pool starts");
    let limits = RunLimits {
        time: Duration::from_secs(1),
        ..RunLimits::default()
    };
    let mut covered = BTreeSet::new();
    let mut calls = 0usize;
    let mut slowest = (Duration::ZERO, String::new());
    let mut every = signatures(&engine);
    every.extend(operator_signatures());
    for signature in every {
        let pools: Option<Vec<&'static [&'static str]>> = signature
            .parameters
            .iter()
            .map(|kind| arguments_for(kind))
            .collect();
        let Some(pools) = pools else { continue };
        for arguments in combinations(&pools) {
            let source = call_source(&signature.name, &arguments);
            let job = match sweep_job(&source, limits) {
                Ok(job) => job,
                Err(ScriptError::Compile(_)) => continue,
                Err(error) => panic!("{source}: {error}"),
            };
            let started = Instant::now();
            let outcome = pool.run(job);
            let elapsed = started.elapsed();
            calls += 1;
            covered.insert(signature.name.clone());
            if elapsed > slowest.0 {
                slowest = (elapsed, source.clone());
            }
            match &outcome {
                Err(ScriptError::ResultTimeout | ScriptError::Panic) => {
                    panic!("{source}: did not terminate cleanly: {outcome:?}")
                }
                Err(ScriptError::PoolUnhealthy | ScriptError::QueueTimeout) => {
                    panic!("{source}: the pool lost workers: {outcome:?}")
                }
                _ => {}
            }
        }
    }
    println!(
        "sweep: {calls} calls, slowest {:?} for {}",
        slowest.0, slowest.1
    );
    assert!(calls <= MAX_SWEEP_CALLS, "{calls} calls");
    assert!(calls > 1000, "{calls} calls");
    assert!(
        slowest.0 < Duration::from_secs(4),
        "{:?}: {}",
        slowest.0,
        slowest.1
    );
    let uncovered: Vec<&&str> = ALLOWED_FUNCTIONS
        .iter()
        .filter(|name| !covered.contains(**name))
        .collect();
    println!("uncovered: {uncovered:?}");
    assert!(uncovered.is_empty(), "uncovered: {uncovered:?}");
    assert_eq!(pool.stats().stuck_workers, 0);
}
