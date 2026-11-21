mod common;
mod ruled;
mod served;

use common::Harness;
use ruled::{add_rule, counter, relaxed, respond, rule};
use serde_json::json;
use served::{Configured, endpoint_configured};
use tokio::task::JoinSet;
use whl_capture::RulesSettings;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_captures_each_receive_a_distinct_atomic_counter_value() {
    let harness = Harness::new().await.with_rules(
        RulesSettings {
            per_endpoint: 64,
            ..relaxed()
        },
        None,
    );
    let id = endpoint_configured(&harness, Configured::default()).await;
    add_rule(
        &harness,
        &id,
        rule(
            "count",
            json!({}),
            vec![
                json!({"kind": "increment", "counter": "hits", "by": 1}),
                respond("200", "{{ counter.hits }}"),
            ],
        ),
    )
    .await;

    let mut tasks = JoinSet::new();
    for _ in 0..12 {
        let router = harness.router.clone();
        let path = format!("/{id}");
        tasks.spawn(async move {
            common::send(
                &router,
                common::request("POST", &path)
                    .body(axum::body::Body::from("x"))
                    .expect("request builds"),
            )
            .await
        });
    }
    let mut seen = Vec::new();
    while let Some(reply) = tasks.join_next().await {
        let reply = reply.expect("the request task finishes");
        assert_eq!(reply.status, 200, "{}", reply.body);
        seen.push(reply.body.parse::<i64>().expect("the body is the counter"));
    }

    seen.sort_unstable();
    assert_eq!(seen, (1..=12).collect::<Vec<i64>>());
    assert_eq!(counter(&harness, &id, "hits").await, Some(12));
}

#[tokio::test]
async fn a_stored_variable_is_written_before_the_response_and_read_back_by_the_next_capture() {
    let harness = Harness::new().await.with_rules(relaxed(), None);
    let id = endpoint_configured(&harness, Configured::default()).await;
    add_rule(
        &harness,
        &id,
        rule(
            "remember",
            json!({"value": "request.query.q", "op": "exists"}),
            vec![
                json!({"kind": "set_store", "name": "last", "value": "{{ request.query.q }}"}),
                respond("200", "now {{ store.last }}"),
            ],
        ),
    )
    .await;
    add_rule(
        &harness,
        &id,
        rule(
            "recall",
            json!({"not": {"value": "request.query.q", "op": "exists"}}),
            vec![respond("200", "before {{ store.last }}")],
        ),
    )
    .await;

    let first = harness.get(&format!("/{id}?q=alpha")).await;
    let second = harness.get(&format!("/{id}")).await;

    assert_eq!(first.body, "now alpha");
    assert_eq!(second.body, "before alpha");
    let stored: String = sqlx::query_scalar(
        "SELECT value::text FROM endpoint_vars WHERE endpoint_id = $1 AND name = 'last'",
    )
    .bind(id.unpack())
    .fetch_one(harness.db.pool())
    .await
    .expect("the variable is stored");
    assert_eq!(stored, "\"alpha\"");
}
