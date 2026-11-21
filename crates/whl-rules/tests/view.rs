use serde_json::{Value, json};
use uuid::Uuid;
use whl_rules::{
    EVENT_PAYLOAD_LIMIT, Entry, MAX_LOG_BYTES, Outcome, RunView, StepEntry, fit_entries,
    rule_result_payload,
};

fn step(index: usize, detail: usize, output: usize) -> Entry {
    Entry::Step(StepEntry {
        rule_id: Uuid::from_u128(1),
        step: index,
        name: None,
        kind: "set_var",
        outcome: Outcome::Ok,
        duration_ms: 1,
        detail: Some("d".repeat(detail)),
        output: Some(json!({"value": "o".repeat(output)})),
    })
}

fn view(entries: Vec<Entry>) -> RunView {
    RunView {
        id: Uuid::from_u128(2),
        capture_id: Uuid::from_u128(2),
        trigger: "capture",
        state: "succeeded",
        rules_version: 3,
        response_status: Some(202),
        error: None,
        entries,
        created_at: "2026-11-21T10:00:00.000Z".to_owned(),
        finished_at: Some("2026-11-21T10:00:00.010Z".to_owned()),
    }
}

#[test]
fn a_run_log_larger_than_its_cap_sheds_outputs_then_details_then_entries() {
    let entries: Vec<Entry> = (0..40).map(|index| step(index, 1000, 3000)).collect();

    let fitted = fit_entries(entries, MAX_LOG_BYTES);

    let size = serde_json::to_vec(&fitted).expect("the log encodes").len();
    assert!(size <= MAX_LOG_BYTES, "{size}");
    assert!(!fitted.is_empty());
}

#[test]
fn the_rule_result_event_always_fits_one_notification() {
    let entries: Vec<Entry> = (0..40).map(|index| step(index, 1000, 3000)).collect();

    let payload = rule_result_payload("0123456789abcdefghjkmnpq", &view(entries))
        .expect("the payload encodes");

    assert!(payload.len() <= EVENT_PAYLOAD_LIMIT, "{}", payload.len());
    let parsed: Value = serde_json::from_str(&payload).expect("the payload is JSON");
    assert_eq!(parsed["t"], "rule_result");
    assert_eq!(parsed["d"]["run"]["rules_version"], 3);
    assert_eq!(parsed["d"]["capture_id"], parsed["d"]["run"]["capture_id"]);
}
