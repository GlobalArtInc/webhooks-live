mod common;
mod served;
mod spy;

use std::time::Duration;

use common::Harness;
use serde_json::json;
use served::{Configured, endpoint_configured, make_templated, set_var};
use spy::Spy;
use whl_core::endpoint_id::EndpointId;
use whl_store::Store;

const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

async fn templated_with(harness: &Harness, body: &str) -> EndpointId {
    let id = endpoint_configured(
        harness,
        Configured {
            body: body.as_bytes().to_vec(),
            ..Configured::default()
        },
    )
    .await;
    make_templated(harness, &id, None).await;
    id
}

async fn statements_for(harness: &Harness, spy: &Spy, id: &EndpointId) -> usize {
    spy.clear();
    harness.get(&format!("/{id}")).await;
    spy.statement_count()
}

#[tokio::test]
async fn templates_that_reference_no_variables_cost_no_extra_queries() {
    let harness = Harness::new().await;
    let plain = endpoint_configured(
        &harness,
        Configured {
            body: b"static".to_vec(),
            ..Configured::default()
        },
    )
    .await;
    let no_refs = templated_with(&harness, "{{ request.method }} {{ capture.seq }}").await;
    let one_counter = templated_with(&harness, "{{ counter.hits }}").await;
    let several = templated_with(
        &harness,
        "{{ counter.a }}{{ counter.b }}{{ store.c }}{{ store.d.e }}",
    )
    .await;
    for id in [&plain, &no_refs, &one_counter, &several] {
        harness.get(&format!("/{id}")).await;
    }
    let (spy, _guard) = Spy::install();

    let baseline = statements_for(&harness, &spy, &plain).await;
    let templated = statements_for(&harness, &spy, &no_refs).await;
    let with_counter = statements_for(&harness, &spy, &one_counter).await;
    let with_everything = statements_for(&harness, &spy, &several).await;

    assert!(baseline > 0);
    assert_eq!(templated, baseline);
    assert_eq!(with_counter, baseline + 1);
    assert_eq!(with_everything, baseline + 1);
}

#[tokio::test]
async fn a_counter_reads_the_committed_value_and_a_missing_one_renders_zero() {
    let harness = Harness::new().await;
    let id = templated_with(
        &harness,
        "[{{ counter.hits }}][{{ counter.none }}][{{ store.text }}]",
    )
    .await;
    let store = Store::from_pool(harness.db.pool().clone());
    set_var(&harness, &id, "text", json!("not a counter")).await;
    for _ in 0..3 {
        store
            .increment_counter(&id, RETENTION, "hits")
            .await
            .expect("the counter increments");
    }

    let reply = harness.get(&format!("/{id}")).await;
    store
        .increment_counter(&id, RETENTION, "hits")
        .await
        .expect("the counter increments");
    let again = harness.get(&format!("/{id}")).await;

    assert_eq!(reply.body, "[3][0][not a counter]");
    assert_eq!(again.body, "[4][0][not a counter]");
    assert!(reply.header("x-webhooks-live-template").is_none());
}

#[tokio::test]
async fn a_counter_name_holding_text_renders_as_a_missing_counter_zero() {
    let harness = Harness::new().await;
    let id = templated_with(&harness, "[{{ counter.label }}]").await;
    set_var(&harness, &id, "label", json!("text")).await;

    let reply = harness.get(&format!("/{id}")).await;

    assert_eq!(reply.body, "[0]");
}
