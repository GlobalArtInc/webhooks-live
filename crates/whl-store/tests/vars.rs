use std::time::Duration;

use serde_json::json;
use tokio::task::JoinSet;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_store::vars::{DeleteVarOutcome, IncrementOutcome, MAX_VARS_PER_ENDPOINT, PutVarOutcome};
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const LOAD_DEADLINE: Duration = Duration::from_secs(2);

async fn fixture() -> (TestDb, Store, EndpointId) {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let record = store
        .create_endpoint(
            &EndpointSettings {
                title: None,
                response: ResponseConfig::default(),
                templating: Default::default(),
                cors: false,
                overflow: Overflow::Refuse,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created");
    (db, store, record.id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fifty_concurrent_increments_count_each_one_exactly_once() {
    let (_db, store, endpoint) = fixture().await;
    let mut tasks = JoinSet::new();
    for _ in 0..50 {
        let store = store.clone();
        let endpoint = endpoint.clone();
        tasks.spawn(async move { store.increment_counter(&endpoint, RETENTION, "hits").await });
    }
    let mut seen = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        match joined
            .expect("the task finishes")
            .expect("the increment succeeds")
        {
            IncrementOutcome::Value(value) => seen.push(value),
            other => panic!("unexpected outcome {other:?}"),
        }
    }

    seen.sort_unstable();
    assert_eq!(seen, (1..=50).collect::<Vec<i64>>());
    let loaded = store
        .load_vars(&endpoint, &["hits".to_owned()], LOAD_DEADLINE)
        .await
        .expect("the variables load");
    assert_eq!(loaded.counter("hits"), Some(50));
}

#[tokio::test]
async fn a_variable_holding_text_is_not_a_counter() {
    let (_db, store, endpoint) = fixture().await;
    store
        .put_var(&endpoint, RETENTION, "label", &json!("text"))
        .await
        .expect("the variable is stored");

    let outcome = store
        .increment_counter(&endpoint, RETENTION, "label")
        .await
        .expect("the increment answers");

    assert_eq!(outcome, IncrementOutcome::NotACounter);
}

#[tokio::test]
async fn the_variable_limit_holds_for_puts_and_new_counters_alike() {
    let (_db, store, endpoint) = fixture().await;
    for index in 0..MAX_VARS_PER_ENDPOINT {
        let outcome = store
            .put_var(&endpoint, RETENTION, &format!("v{index}"), &json!(index))
            .await
            .expect("the variable is stored");
        assert!(matches!(outcome, PutVarOutcome::Stored(_)));
    }

    let extra = store
        .put_var(&endpoint, RETENTION, "extra", &json!(1))
        .await
        .expect("the put answers");
    let counter = store
        .increment_counter(&endpoint, RETENTION, "extra")
        .await
        .expect("the increment answers");
    let existing = store
        .put_var(&endpoint, RETENTION, "v0", &json!("changed"))
        .await
        .expect("the put answers");

    assert_eq!(extra, PutVarOutcome::LimitReached);
    assert_eq!(counter, IncrementOutcome::LimitReached);
    assert!(matches!(existing, PutVarOutcome::Stored(_)));
}

#[tokio::test]
async fn loading_returns_only_the_requested_names() {
    let (_db, store, endpoint) = fixture().await;
    for name in ["a", "b", "c"] {
        store
            .put_var(&endpoint, RETENTION, name, &json!({ "name": name }))
            .await
            .expect("the variable is stored");
    }

    let loaded = store
        .load_vars(&endpoint, &["a".to_owned(), "c".to_owned()], LOAD_DEADLINE)
        .await
        .expect("the variables load");

    let names: Vec<&str> = loaded.values().keys().map(String::as_str).collect();
    assert_eq!(names, ["a", "c"]);
    assert_eq!(loaded.values()["c"], json!({"name": "c"}));
}

#[tokio::test]
async fn deleting_reports_missing_variables_and_gone_endpoints() {
    let (_db, store, endpoint) = fixture().await;
    store
        .put_var(&endpoint, RETENTION, "gone_soon", &json!(true))
        .await
        .expect("the variable is stored");

    let first = store
        .delete_var(&endpoint, RETENTION, "gone_soon")
        .await
        .expect("the delete answers");
    let second = store
        .delete_var(&endpoint, RETENTION, "gone_soon")
        .await
        .expect("the delete answers");
    let unknown = EndpointId::generate().expect("an id");
    let elsewhere = store
        .delete_var(&unknown, RETENTION, "gone_soon")
        .await
        .expect("the delete answers");

    assert_eq!(first, DeleteVarOutcome::Deleted);
    assert_eq!(second, DeleteVarOutcome::NotFound);
    assert_eq!(elsewhere, DeleteVarOutcome::EndpointGone);
}
