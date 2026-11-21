mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{
    Recorder, compile, context, machine, respond, rule, rule_entries, rule_id, run, run_with,
    step_entries,
};
use serde_json::{Value, json};
use whl_rules::{
    Advance, DriveEnv, FailureCode, HeaderOp, Mode, Outcome, RunOutcome, STATE_STEP_TIMEOUT,
    SliceHook, StateError, StoreCall, drive,
};
use whl_script::DEFAULT_MEMORY_BYTES;

fn always() -> Value {
    json!({})
}

fn failure_of(finished: &whl_rules::Finished) -> FailureCode {
    match &finished.outcome {
        RunOutcome::Failed(failure) => failure.code,
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
}

#[test]
fn the_cut_follows_the_last_response_step_of_the_last_rule_that_has_one() {
    let rules = compile(vec![
        rule(
            "first",
            always(),
            vec![
                json!({"kind": "set_var", "into": "a", "value": "1"}),
                respond("200", "one"),
                json!({"kind": "set_var", "into": "b", "value": "2"}),
            ],
        ),
        rule(
            "second",
            always(),
            vec![
                json!({"kind": "set_var", "into": "c", "value": "3"}),
                json!({"kind": "set_header", "name": "X-A", "value": "1"}),
                json!({"kind": "set_var", "into": "d", "value": "4"}),
            ],
        ),
        rule(
            "third",
            always(),
            vec![json!({"kind": "set_var", "into": "e", "value": "5"})],
        ),
    ]);

    let cut = rules.plan().cut().expect("a response step exists");

    assert_eq!((cut.rule, cut.step), (1, 1));
}

#[tokio::test]
async fn a_rule_set_without_a_response_step_runs_nothing_before_the_response() {
    let rules = compile(vec![rule(
        "count",
        always(),
        vec![json!({"kind": "increment", "counter": "hits", "by": 1})],
    )]);
    let state = Recorder::default();

    let finished = run_with(&rules, context(), Mode::Live, &state, None).await;

    assert!(finished.log.is_empty());
    assert!(state.calls().is_empty());
    assert!(finished.response.is_untouched());
    let resume = finished.after.resume.expect("everything is left for later");
    assert_eq!((resume.rule, resume.step), (0, 0));
}

#[tokio::test]
async fn a_respond_step_renders_the_response_from_the_request() {
    let rules = compile(vec![rule(
        "answer",
        always(),
        vec![json!({
            "kind": "respond",
            "status": "202",
            "content_type": "application/json",
            "headers": [{"name": "X-Echo", "value": "{{ request.header.x-token }}"}],
            "body": "{\"id\": {{ json(\"$.order.id\") }}, \"page\": \"{{ request.query.page }}\"}",
        })],
    )]);

    let finished = run(&rules, context()).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(finished.response.status, Some(202));
    assert_eq!(
        finished.response.content_type.as_deref(),
        Some("application/json")
    );
    assert_eq!(
        finished.response.body.as_deref(),
        Some(br#"{"id": 42, "page": "3"}"#.as_slice())
    );
    assert_eq!(
        finished.response.header_ops,
        vec![HeaderOp::Add("X-Echo".to_owned(), "abc".to_owned())]
    );
    assert!(finished.after.is_empty());
}

#[tokio::test]
async fn header_steps_edit_the_base_headers_and_a_later_respond_replaces_them() {
    let rules = compile(vec![rule(
        "headers",
        always(),
        vec![
            json!({"kind": "set_header", "name": "X-Keep", "value": "new"}),
            json!({"kind": "remove_header", "name": "X-Drop"}),
            respond("200", ""),
        ],
    )]);
    let base = vec![
        ("X-Keep".to_owned(), "old".to_owned()),
        ("X-Drop".to_owned(), "gone".to_owned()),
        ("X-Other".to_owned(), "stays".to_owned()),
    ];

    let finished = run(&rules, context()).await;
    let after_respond = finished.response.apply_headers(base);

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert!(after_respond.is_empty());
}

#[tokio::test]
async fn a_header_step_alone_keeps_the_base_headers_it_does_not_touch() {
    let rules = compile(vec![rule(
        "headers",
        always(),
        vec![
            json!({"kind": "set_header", "name": "x-keep", "value": "new"}),
            json!({"kind": "remove_header", "name": "X-Drop"}),
            json!({"kind": "delay", "ms": 5}),
            json!({"kind": "stop"}),
            respond("200", ""),
        ],
    )]);
    let base = vec![
        ("X-Keep".to_owned(), "old".to_owned()),
        ("X-Drop".to_owned(), "gone".to_owned()),
        ("X-Other".to_owned(), "stays".to_owned()),
    ];

    let finished = run(&rules, context()).await;

    assert_eq!(
        finished.response.apply_headers(base),
        vec![
            ("X-Other".to_owned(), "stays".to_owned()),
            ("x-keep".to_owned(), "new".to_owned()),
        ]
    );
    assert_eq!(finished.response.delay_ms, 5);
    assert!(finished.stopped);
}

#[tokio::test]
async fn a_render_error_fails_the_run_with_the_template_code() {
    let rules = compile(vec![rule(
        "answer",
        always(),
        vec![respond("200", "{{ request.query.page | base64_decode }}")],
    )]);
    let mut broken = context();
    broken.request.query = vec![whl_template::NameValue::new("page", "!!")];

    let finished = run(&rules, broken).await;

    assert_eq!(failure_of(&finished), FailureCode::Template);
    assert!(finished.after.is_empty());
    let steps = step_entries(&finished);
    assert_eq!(steps[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn a_failing_step_marked_continue_on_error_lets_the_run_go_on() {
    let rules = compile(vec![rule(
        "answer",
        always(),
        vec![
            json!({
                "kind": "set_var",
                "into": "x",
                "value": "{{ request.query.page | base64_decode }}",
                "continue_on_error": true,
                "name": "first",
            }),
            respond("201", "{{ step.first.error | default(\"none\") }}"),
        ],
    )]);
    let mut broken = context();
    broken.request.query = vec![whl_template::NameValue::new("page", "!!")];

    let finished = run(&rules, broken).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(finished.response.status, Some(201));
    assert_eq!(
        finished.response.body.as_deref(),
        Some(b"template".as_slice())
    );
    assert_eq!(step_entries(&finished)[0].outcome, Outcome::Failed);
}

#[tokio::test]
async fn stop_skips_later_rules_and_leaves_nothing_for_after_the_response() {
    let rules = compile(vec![
        rule(
            "first",
            always(),
            vec![respond("201", "first"), json!({"kind": "stop"})],
        ),
        rule("second", always(), vec![respond("202", "second")]),
    ]);

    let finished = run(&rules, context()).await;

    assert!(finished.stopped);
    assert_eq!(finished.response.status, Some(201));
    assert_eq!(rule_entries(&finished).len(), 1);
    assert!(finished.after.is_empty());
}

#[tokio::test]
async fn the_last_response_step_wins_across_rules() {
    let rules = compile(vec![
        rule("first", always(), vec![respond("201", "first")]),
        rule("second", always(), vec![respond("202", "second")]),
    ]);

    let finished = run(&rules, context()).await;

    assert_eq!(finished.response.status, Some(202));
    assert_eq!(
        finished.response.body.as_deref(),
        Some(b"second".as_slice())
    );
}

#[tokio::test]
async fn compute_steps_feed_the_templates_of_later_steps() {
    let rules = compile(vec![rule(
        "compute",
        always(),
        vec![
            json!({"kind": "extract", "from": "request.body", "json_path": "$.order.tag", "into": "tag"}),
            json!({"kind": "extract", "from": "request.path", "regex": "^/orders/([0-9]+)$", "into": "number"}),
            json!({"kind": "set_var", "into": "label", "value": "{{ var.tag | upper }}-{{ var.number }}"}),
            json!({"kind": "transform", "into": "payload", "template": "payload for {{ var.label }}"}),
            respond("200", "{{ var.label }}"),
        ],
    )]);

    let finished = run(&rules, context()).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(
        finished.response.body.as_deref(),
        Some(b"GIFT-42".as_slice())
    );
    assert_eq!(finished.vars["label"], "GIFT-42");
    assert_eq!(
        finished.after.payload.as_deref(),
        Some("payload for GIFT-42")
    );
}

#[tokio::test]
async fn an_extract_without_a_match_sets_nothing_and_says_so() {
    let rules = compile(vec![rule(
        "compute",
        always(),
        vec![
            json!({"kind": "extract", "from": "request.path", "regex": "^/none/([0-9]+)$", "into": "number"}),
            respond("200", "[{{ var.number }}]"),
        ],
    )]);

    let finished = run(&rules, context()).await;

    assert!(finished.vars.is_empty());
    assert_eq!(
        step_entries(&finished)[0].detail.as_deref(),
        Some("no match")
    );
    assert_eq!(finished.response.body.as_deref(), Some(b"[]".as_slice()));
}

#[tokio::test]
async fn state_steps_call_the_backend_and_feed_later_templates() {
    let rules = compile(vec![rule(
        "state",
        always(),
        vec![
            json!({"kind": "set_store", "name": "last", "value": "{{ request.query.page }}"}),
            json!({"kind": "increment", "counter": "hits", "by": 5}),
            respond("200", "{{ counter.hits }}/{{ store.last }}"),
        ],
    )]);
    let state = Recorder::default();

    let finished = run_with(&rules, context(), Mode::Live, &state, None).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(
        state.calls(),
        vec![
            StoreCall::Set {
                name: "last".to_owned(),
                value: json!(3),
            },
            StoreCall::Add {
                name: "hits".to_owned(),
                by: 5,
            },
        ]
    );
    assert_eq!(finished.response.body.as_deref(), Some(b"5/3".as_slice()));
}

#[tokio::test]
async fn a_state_error_fails_the_run_and_an_unreachable_store_is_internal() {
    let rules = compile(vec![rule(
        "state",
        always(),
        vec![
            json!({"kind": "increment", "counter": "label", "by": 1}),
            respond("200", "x"),
        ],
    )]);

    let not_a_counter = run_with(
        &rules,
        context(),
        Mode::Live,
        &Recorder::failing(StateError::NotACounter),
        None,
    )
    .await;
    let unreachable = run_with(
        &rules,
        context(),
        Mode::Live,
        &Recorder::failing(StateError::Timeout),
        None,
    )
    .await;

    assert_eq!(failure_of(&not_a_counter), FailureCode::Template);
    assert_eq!(failure_of(&unreachable), FailureCode::Internal);
}

#[tokio::test]
async fn side_effect_steps_before_the_cut_are_deferred_with_a_snapshot_and_the_rest_is_planned() {
    let rules = compile(vec![
        rule(
            "first",
            always(),
            vec![
                json!({"kind": "set_var", "into": "who", "value": "{{ request.ip }}"}),
                json!({"kind": "forward", "target_url": "https://example.test/hook"}),
                respond("200", "ok"),
                json!({"kind": "set_var", "into": "late", "value": "1"}),
            ],
        ),
        rule(
            "second",
            always(),
            vec![json!({"kind": "set_var", "into": "other", "value": "2"})],
        ),
    ]);

    let finished = run(&rules, context()).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(finished.after.deferred.len(), 1);
    let deferred = &finished.after.deferred[0];
    assert_eq!(deferred.rule_id, rule_id(0));
    assert_eq!(deferred.step, 1);
    assert_eq!(
        finished.after.snapshots[deferred.snapshot].vars["who"],
        "203.0.113.9"
    );
    assert_eq!(step_entries(&finished)[1].outcome, Outcome::Deferred);
    let plan = finished
        .after
        .plan(rules.rules())
        .expect("the plan encodes");
    assert_eq!(plan["version"], 1);
    assert_eq!(plan["resume"], json!({"rule": 0, "step": 3}));
    assert_eq!(plan["rules"].as_array().map(Vec::len), Some(2));
    assert_eq!(plan["rules"][0]["name"], "first");
    assert_eq!(plan["deferred"][0]["action"]["kind"], "forward");
    assert_eq!(plan["snapshots"][0]["vars"]["who"], "203.0.113.9");
}

#[tokio::test]
async fn deferred_steps_share_one_snapshot_until_the_variables_change() {
    let mut steps = vec![json!({"kind": "set_var", "into": "big", "value": "{{ request.body }}"})];
    for _ in 0..9 {
        steps.push(json!({"kind": "forward", "target_url": "https://example.test/hook"}));
    }
    let mut last = vec![json!({"kind": "set_var", "into": "other", "value": "1"})];
    for _ in 0..8 {
        last.push(json!({"kind": "forward", "target_url": "https://example.test/hook"}));
    }
    last.push(respond("200", "ok"));
    let rules = compile(vec![
        rule("first", always(), steps),
        rule("second", always(), last),
    ]);
    let mut request = context();
    request.request.body = whl_template::BodyData::from_text(&"x".repeat(15 * 1024));

    let finished = run(&rules, request).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert_eq!(finished.after.deferred.len(), 17);
    assert_eq!(finished.after.snapshots.len(), 2);
    let plan = finished.after.plan(rules.rules()).expect("the plan fits");
    assert_eq!(plan["snapshots"].as_array().map(Vec::len), Some(2));
    assert_eq!(plan["deferred"][0]["snapshot"], 0);
    assert_eq!(plan["deferred"][16]["snapshot"], 1);
}

#[tokio::test]
async fn a_dry_run_reports_state_writes_without_performing_them() {
    let rules = compile(vec![rule(
        "state",
        always(),
        vec![
            json!({"kind": "increment", "counter": "hits", "by": 2}),
            respond("200", "{{ counter.hits }}"),
        ],
    )]);
    let state = Recorder::default();

    let finished = run_with(&rules, context(), Mode::DryRun, &state, None).await;

    assert_eq!(finished.outcome, RunOutcome::Succeeded);
    assert!(state.calls().is_empty());
    assert_eq!(step_entries(&finished)[0].outcome, Outcome::NoOp);
    assert_eq!(finished.response.body.as_deref(), Some(b"2".as_slice()));
}

#[tokio::test]
async fn an_http_step_before_the_response_fails_with_the_http_code() {
    let rules = compile(vec![rule(
        "fetch",
        always(),
        vec![
            json!({"kind": "http", "method": "GET", "url": "https://example.test/"}),
            respond("200", "x"),
        ],
    )]);

    let live = run(&rules, context()).await;
    let dry = run_with(&rules, context(), Mode::DryRun, &Recorder::default(), None).await;

    assert_eq!(failure_of(&live), FailureCode::Http);
    assert_eq!(failure_of(&dry), FailureCode::Http);
}

#[tokio::test]
async fn a_slow_render_hits_the_run_budget_and_a_spent_budget_cannot_be_forgiven() {
    let heavy = "{{ request.body | base64 | hex | sha256 | hex | base64 | hex }}".repeat(400);
    let rules = compile(vec![rule(
        "heavy",
        always(),
        vec![
            json!({"kind": "set_var", "into": "x", "value": heavy, "continue_on_error": true}),
            respond("200", "x"),
        ],
    )]);
    let mut big = context();
    big.request.body = whl_template::BodyData::from_text(&"x".repeat(60 * 1024));
    let mut running = machine(&rules, big, Mode::Live, Duration::from_millis(1));

    let finished = match running.advance() {
        Advance::Finished(finished) => finished,
        Advance::Effect(_) => panic!("no effect is expected"),
        Advance::Checkpoint => panic!("no checkpoint is expected"),
    };

    assert_eq!(failure_of(&finished), FailureCode::Budget);
}

struct Released(Arc<AtomicBool>);

impl Drop for Released {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn a_timed_out_run_keeps_its_guard_until_the_blocking_work_ends() {
    let rules = compile(vec![rule("answer", always(), vec![respond("200", "x")])]);
    let released = Arc::new(AtomicBool::new(false));
    let hook: SliceHook = Arc::new(|| std::thread::sleep(Duration::from_millis(400)));
    let state = Recorder::default();
    let env = DriveEnv {
        pool: None,
        state: &state,
        script_memory_bytes: DEFAULT_MEMORY_BYTES,
        state_timeout: STATE_STEP_TIMEOUT,
        slice_hook: Some(hook),
    };

    let finished = drive(
        machine(&rules, context(), Mode::Live, Duration::from_secs(5)),
        &env,
        Duration::from_millis(50),
        Released(Arc::clone(&released)),
    )
    .await;

    assert_eq!(failure_of(&finished), FailureCode::Budget);
    assert!(
        !released.load(Ordering::SeqCst),
        "the guard was released while the blocking work still ran"
    );
    for _ in 0..200 {
        if released.load(Ordering::SeqCst) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the guard was never released after the blocking work ended");
}
