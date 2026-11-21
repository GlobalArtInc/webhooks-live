mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use common::{compiler, context, rule, rule_id, step_entries};
use serde_json::{Value, json};
use uuid::Uuid;
use whl_rules::{
    Action, Advance, Answer, CompiledRules, Effect, Entry, Failure, FailureCode, Finished, Machine,
    MachineInput, Mode, OutboundCall, OutboundDone, OutboundStep, Outcome, References, Resumption,
    RuleSource, RunOutcome, STEP_BUDGET, deferred_from_plan, deferred_step, entries_from_value,
    plan_payload, plan_resume_step, snapshots_from_plan, steps_from_value, steps_to_value,
    validate_set,
};
use whl_template::{Context, NameValue, RandomSeed, Secrets, StepData};

const CHANNEL: &str = "0190c3a2-7d1e-7000-8000-000000000002";

struct Report {
    finished: Finished,
    checkpoints: Vec<usize>,
    sent: Vec<OutboundStep>,
}

fn compile_with(inputs: Vec<Value>, secrets: &[&str]) -> Arc<CompiledRules> {
    let scripts = compiler();
    let references = References {
        secrets: secrets.iter().map(|name| (*name).to_owned()).collect(),
        forward_rules: BTreeSet::new(),
    };
    let valid = validate_set(&inputs, &references, &scripts).expect("the rules are valid");
    let sources = valid
        .into_iter()
        .enumerate()
        .map(|(index, rule)| RuleSource {
            id: rule_id(index),
            rule,
        })
        .collect();
    Arc::new(CompiledRules::compile(sources, &scripts).expect("the rules compile"))
}

fn after_machine(rules: &Arc<CompiledRules>, context: Context, resumption: Resumption) -> Machine {
    Machine::resumed(
        MachineInput {
            rules: Arc::clone(rules),
            context,
            seed: RandomSeed::from_bytes(b"after seed"),
            budget: STEP_BUDGET,
            mode: Mode::After,
            tenant: "ep".to_owned(),
        },
        resumption,
    )
}

fn run_after(
    rules: &Arc<CompiledRules>,
    context: Context,
    position: usize,
    answer: impl Fn(&OutboundStep) -> Result<OutboundDone, Failure>,
) -> Report {
    let machine = after_machine(
        rules,
        context,
        Resumption {
            position,
            ..Resumption::default()
        },
    );
    drive(machine, answer)
}

fn drive_resumed(machine: Machine) -> Report {
    drive(machine, |_| done(200))
}

fn drive(
    mut machine: Machine,
    answer: impl Fn(&OutboundStep) -> Result<OutboundDone, Failure>,
) -> Report {
    let mut checkpoints = Vec::new();
    let mut sent = Vec::new();
    loop {
        match machine.advance() {
            Advance::Checkpoint => checkpoints.push(machine.position()),
            Advance::Effect(Effect::Outbound(step)) => {
                let result = answer(&step);
                sent.push(*step);
                machine.resume(Answer::Outbound(Box::new(result)));
            }
            Advance::Effect(_) => panic!("no local effect is expected"),
            Advance::Finished(finished) => {
                return Report {
                    finished: *finished,
                    checkpoints,
                    sent,
                };
            }
        }
    }
}

fn done(status: u16) -> Result<OutboundDone, Failure> {
    Ok(OutboundDone {
        detail: None,
        output: json!({"status": status}),
        data: Some(StepData {
            status: Some(status),
            headers: vec![NameValue::new("content-type", "text/plain")],
            body: "ok".to_owned(),
            duration_ms: 0,
            error: None,
        }),
    })
}

fn never_called(_: &OutboundStep) -> Result<OutboundDone, Failure> {
    panic!("no outbound call is expected")
}

fn fake_secret() -> String {
    ["tok", "en-de", "mo-1"].concat()
}

#[test]
fn after_mode_logs_response_steps_as_no_ops_and_runs_every_other_step_in_order() {
    let rules = compile_with(
        vec![rule(
            "all",
            json!({}),
            vec![
                json!({"kind": "respond", "status": "200", "body": "late"}),
                json!({"kind": "set_var", "into": "a", "value": "1"}),
                json!({"kind": "set_header", "name": "X-A", "value": "1"}),
                json!({"kind": "set_var", "into": "b", "value": "{{ var.a }}2"}),
            ],
        )],
        &[],
    );

    let report = run_after(&rules, context(), 0, never_called);

    assert_eq!(report.finished.outcome, RunOutcome::Succeeded);
    let outcomes: Vec<Outcome> = step_entries(&report.finished)
        .iter()
        .map(|entry| entry.outcome)
        .collect();
    assert_eq!(
        outcomes,
        [Outcome::NoOp, Outcome::Ok, Outcome::NoOp, Outcome::Ok]
    );
    assert_eq!(report.finished.vars.get("b"), Some(&json!("12")));
    assert!(report.finished.response.is_untouched());
}

#[test]
fn a_checkpoint_follows_every_step_and_every_skipped_rule() {
    let rules = compile_with(
        vec![
            rule(
                "skipped",
                json!({"value": "request.method", "op": "eq", "arg": "GET"}),
                vec![
                    json!({"kind": "set_var", "into": "a", "value": "1"}),
                    json!({"kind": "set_var", "into": "b", "value": "2"}),
                ],
            ),
            rule(
                "taken",
                json!({}),
                vec![
                    json!({"kind": "set_var", "into": "c", "value": "3"}),
                    json!({"kind": "set_var", "into": "d", "value": "4"}),
                ],
            ),
        ],
        &[],
    );

    let report = run_after(&rules, context(), 0, never_called);

    assert_eq!(report.checkpoints, [2, 3, 4]);
    assert_eq!(report.finished.vars.get("a"), None);
    assert_eq!(report.finished.vars.get("d"), Some(&json!("4")));
}

#[test]
fn a_machine_resumes_at_a_flat_position_without_repeating_earlier_work() {
    let rules = compile_with(
        vec![
            rule(
                "first",
                json!({}),
                vec![
                    json!({"kind": "set_var", "into": "x", "value": "again"}),
                    json!({"kind": "set_var", "into": "y", "value": "{{ var.x }}-y"}),
                ],
            ),
            rule(
                "second",
                json!({}),
                vec![json!({"kind": "set_var", "into": "z", "value": "3"})],
            ),
        ],
        &[],
    );
    let mut resumed = context();
    resumed.vars.insert("x".to_owned(), json!("kept"));

    let report = run_after(&rules, resumed, 1, never_called);

    let indexes: Vec<usize> = step_entries(&report.finished)
        .iter()
        .map(|entry| entry.step)
        .collect();
    assert_eq!(indexes, [1, 0]);
    assert!(
        report
            .finished
            .log
            .iter()
            .all(|entry| !matches!(entry, Entry::Rule(rule) if rule.rule_name == "first"))
    );
    assert_eq!(report.finished.vars.get("y"), Some(&json!("kept-y")));
}

#[test]
fn an_http_step_is_rendered_with_its_secret_and_its_answer_feeds_later_steps() {
    let rules = compile_with(
        vec![rule(
            "call",
            json!({}),
            vec![
                json!({
                    "name": "fetch", "kind": "http", "method": "post",
                    "url": "https://hooks.example.com/{{ request.query.page }}",
                    "headers": [{"name": "Authorization", "value": "Bearer {{ secret.API_KEY }}"}],
                    "body": "{\"id\": {{ json(\"$.order.id\") }}}",
                }),
                json!({"kind": "set_var", "into": "code", "value": "{{ step.fetch.status }}"}),
            ],
        )],
        &["API_KEY"],
    );
    let mut with_secret = context();
    with_secret.secrets = Secrets::new(BTreeMap::from([("API_KEY".to_owned(), fake_secret())]));

    let report = run_after(&rules, with_secret, 0, |_| done(201));

    let [step] = report.sent.as_slice() else {
        panic!("one outbound step is expected");
    };
    let OutboundCall::Http(call) = &step.call else {
        panic!("an HTTP call is expected");
    };
    assert_eq!(call.method, "POST");
    assert_eq!(call.url, "https://hooks.example.com/3");
    assert_eq!(
        call.headers,
        [(
            "Authorization".to_owned(),
            format!("Bearer {}", fake_secret())
        )]
    );
    assert_eq!(call.body.as_deref(), Some(r#"{"id": 42}"#));
    assert_eq!(report.finished.vars.get("code"), Some(&json!("201")));
    let logged = serde_json::to_string(&report.finished.log).expect("the log encodes");
    assert!(!logged.contains(&fake_secret()));
}

#[test]
fn an_http_step_with_an_undefined_secret_fails_instead_of_sending_an_empty_value() {
    let rules = compile_with(
        vec![rule(
            "call",
            json!({}),
            vec![json!({
                "kind": "http", "method": "get",
                "url": "https://hooks.example.com/{{ secret.API_KEY }}",
            })],
        )],
        &["API_KEY"],
    );

    let report = run_after(&rules, context(), 0, never_called);

    match &report.finished.outcome {
        RunOutcome::Failed(failure) => assert_eq!(failure.code, FailureCode::Template),
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
    assert!(report.sent.is_empty());
}

#[test]
fn a_failed_outbound_step_stops_the_run_unless_it_continues_on_error() {
    let failing = |continue_on_error: bool| {
        compile_with(
            vec![rule(
                "call",
                json!({}),
                vec![
                    json!({
                        "name": "fetch", "kind": "http", "method": "get",
                        "url": "https://hooks.example.com/",
                        "continue_on_error": continue_on_error,
                    }),
                    json!({"kind": "set_var", "into": "after", "value": "{{ step.fetch.error }}"}),
                ],
            )],
            &[],
        )
    };
    let refuse = |_: &OutboundStep| Err(Failure::new(FailureCode::Http, "connect_error: refused"));

    let stopped = run_after(&failing(false), context(), 0, refuse);
    let forgiven = run_after(&failing(true), context(), 0, refuse);

    match &stopped.finished.outcome {
        RunOutcome::Failed(failure) => assert_eq!(failure.code, FailureCode::Http),
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
    assert!(stopped.finished.vars.get("after").is_none());
    assert_eq!(forgiven.finished.outcome, RunOutcome::Succeeded);
    assert_eq!(forgiven.finished.vars.get("after"), Some(&json!("http")));
}

#[test]
fn a_notify_step_carries_the_rendered_message_and_a_forward_step_its_payload() {
    let rules = compile_with(
        vec![rule(
            "side effects",
            json!({}),
            vec![
                json!({"kind": "transform", "into": "payload", "template": "shaped {{ json(\"$.order.id\") }}"}),
                json!({"kind": "notify", "channel_id": CHANNEL, "message": "Order {{ json(\"$.order.id\") }}"}),
                json!({"kind": "forward", "target_url": "https://hooks.example.com/in", "body_from": "payload"}),
            ],
        )],
        &[],
    );

    let report = run_after(&rules, context(), 0, |_| {
        Ok(OutboundDone {
            detail: None,
            output: json!({}),
            data: None,
        })
    });

    let calls: Vec<&OutboundCall> = report.sent.iter().map(|step| &step.call).collect();
    let [OutboundCall::Notify(notify), OutboundCall::Forward(forward)] = calls.as_slice() else {
        panic!("a notification and a forward are expected");
    };
    assert_eq!(notify.channel_id, Uuid::parse_str(CHANNEL).expect("a UUID"));
    assert_eq!(notify.message.as_deref(), Some("Order 42"));
    assert_eq!(forward.payload.as_deref(), Some("shaped 42"));
    assert_eq!(report.sent[1].position, 2);
}

#[test]
fn a_forward_step_that_sends_the_payload_fails_when_no_transform_wrote_it() {
    let rules = compile_with(
        vec![rule(
            "forward",
            json!({}),
            vec![
                json!({"kind": "forward", "target_url": "https://hooks.example.com/in", "body_from": "payload"}),
            ],
        )],
        &[],
    );

    let report = run_after(&rules, context(), 0, never_called);

    match &report.finished.outcome {
        RunOutcome::Failed(failure) => assert_eq!(failure.code, FailureCode::Template),
        RunOutcome::Succeeded => panic!("the run succeeded"),
    }
}

#[test]
fn a_stop_step_still_ends_an_after_run() {
    let rules = compile_with(
        vec![rule(
            "halt",
            json!({}),
            vec![
                json!({"kind": "set_var", "into": "a", "value": "1"}),
                json!({"kind": "stop"}),
                json!({"kind": "set_var", "into": "b", "value": "2"}),
            ],
        )],
        &[],
    );

    let report = run_after(&rules, context(), 0, never_called);

    assert!(report.finished.stopped);
    assert_eq!(report.finished.vars.get("b"), None);
}

#[test]
fn stored_run_state_survives_a_round_trip_through_json() {
    let rules = compile_with(
        vec![rule(
            "call",
            json!({}),
            vec![
                json!({"name": "fetch", "kind": "http", "method": "get", "url": "https://hooks.example.com/"}),
            ],
        )],
        &[],
    );
    let report = run_after(&rules, context(), 0, |_| done(200));
    let mut machine = after_machine(&rules, context(), Resumption::default());
    let snapshot = loop {
        match machine.advance() {
            Advance::Checkpoint => {}
            Advance::Effect(Effect::Outbound(_)) => {
                machine.resume(Answer::Outbound(Box::new(done(200))));
            }
            Advance::Effect(_) => panic!("no local effect is expected"),
            Advance::Finished(_) => break machine.progress(),
        }
    };

    let log = serde_json::to_value(&report.finished.log).expect("the log encodes");
    let restored = entries_from_value(&log).expect("the log decodes");
    let steps = steps_from_value(&steps_to_value(&snapshot.steps)).expect("the results decode");

    assert_eq!(restored, report.finished.log);
    assert_eq!(steps.get("fetch").and_then(|data| data.status), Some(200));
    assert_eq!(
        steps.get("fetch").map(|data| data.body.as_str()),
        Some("ok")
    );
}

#[test]
fn the_plan_readers_return_the_deferred_steps_their_snapshots_the_resume_step_and_the_payload() {
    let plan = json!({
        "version": 1,
        "rules": [],
        "resume": {"rule": 0, "step": 2},
        "snapshots": [{"vars": {"a": "1"}, "payload": "early"}],
        "deferred": [{
            "rule_id": "0190c3a2-7d1e-7000-8000-000000000003",
            "rule_name": "notify",
            "step": 1,
            "action": {
                "kind": "notify", "continue_on_error": false,
                "channel_id": CHANNEL, "message": null,
            },
            "snapshot": 0,
        }],
        "payload": "cut",
    });

    let deferred = deferred_from_plan(&plan).expect("the deferred steps are readable");
    let snapshots = snapshots_from_plan(&plan).expect("the snapshots are readable");
    let step = deferred_step(&deferred[0], &compiler()).expect("the step validates");

    assert_eq!(deferred.len(), 1);
    assert_eq!(deferred[0].step, 1);
    assert_eq!(
        snapshots[deferred[0].snapshot].payload.as_deref(),
        Some("early")
    );
    assert_eq!(snapshots[0].vars.get("a"), Some(&json!("1")));
    assert!(matches!(step.action, Action::Notify { .. }));
    assert_eq!(plan_resume_step(&plan), 2);
    assert_eq!(plan_payload(&plan).as_deref(), Some("cut"));
}

#[test]
fn a_machine_resumed_at_the_first_step_of_an_entered_rule_does_not_enter_it_again() {
    let rules = compile_with(
        vec![rule(
            "call",
            json!({}),
            vec![
                json!({"name": "fetch", "kind": "http", "method": "get", "url": "https://hooks.example.com/"}),
            ],
        )],
        &[],
    );
    let mut machine = after_machine(&rules, context(), Resumption::default());
    let Advance::Effect(Effect::Outbound(_)) = machine.advance() else {
        panic!("the HTTP step is asked for");
    };
    let parked = machine.progress();

    let resumed = after_machine(
        &rules,
        context(),
        Resumption {
            position: parked.position,
            log: parked.log,
            ..Resumption::default()
        },
    );
    let report = drive_resumed(resumed);

    let entered: usize = report
        .finished
        .log
        .iter()
        .filter(|entry| matches!(entry, Entry::Rule(_)))
        .count();
    assert_eq!(entered, 1);
    assert_eq!(report.sent.len(), 1);
}
