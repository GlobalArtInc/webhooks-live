use std::time::Duration;

use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_rules::{Action, Condition, Rule, Step};
use whl_store::rules::{RuleChange, WriteOutcome};
use whl_store::{EndpointSettings, Overflow, Store, Uuid};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);

struct Fixture {
    db: TestDb,
    store: Store,
    endpoint: EndpointId,
}

async fn fixture() -> Fixture {
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
                tags: Vec::new(),
                verifier: None,
            },
            RETENTION,
        )
        .await
        .expect("the endpoint is created");
    Fixture {
        db,
        store,
        endpoint: record.id,
    }
}

fn rule(name: &str, action: Action) -> Rule {
    Rule {
        name: name.to_owned(),
        enabled: true,
        condition: Condition::Always,
        steps: vec![Step {
            name: None,
            continue_on_error: false,
            action,
        }],
    }
}

fn respond() -> Action {
    Action::Respond {
        status: "200".to_owned(),
        content_type: None,
        headers: Vec::new(),
        body: String::new(),
    }
}

fn counter() -> Action {
    Action::Increment {
        counter: "hits".to_owned(),
        by: 1,
    }
}

impl Fixture {
    async fn write(&self, expected: i32, change: RuleChange) -> WriteOutcome {
        self.store
            .write_rules(&self.endpoint, RETENTION, expected, &change)
            .await
            .expect("the write runs")
    }

    async fn flags(&self) -> (i32, i16, bool) {
        sqlx::query_as("SELECT rules_version, rule_count, rules_sync FROM endpoints WHERE id = $1")
            .bind(self.endpoint.unpack())
            .fetch_one(self.db.pool())
            .await
            .expect("the flags are readable")
    }
}

fn written(outcome: WriteOutcome) -> whl_store::rules::RuleSet {
    match outcome {
        WriteOutcome::Written(set) => set,
        other => panic!("the write was not applied: {other:?}"),
    }
}

#[tokio::test]
async fn endpoint_flags_follow_the_rule_set_in_the_same_write() {
    let fixture = fixture().await;
    let set = written(
        fixture
            .write(0, RuleChange::Create(rule("answer", respond())))
            .await,
    );
    assert_eq!(set.version, 1);
    assert_eq!(fixture.flags().await, (1, 1, true));
    let id = set.rules[0].id;
    written(fixture.write(1, RuleChange::Remove(id)).await);
    assert_eq!(fixture.flags().await, (2, 0, false));
}

#[tokio::test]
async fn a_rule_without_a_response_step_leaves_rules_sync_off() {
    let fixture = fixture().await;
    written(
        fixture
            .write(0, RuleChange::Create(rule("count", counter())))
            .await,
    );
    assert_eq!(fixture.flags().await, (1, 1, false));
}

#[tokio::test]
async fn a_stale_version_writes_nothing() {
    let fixture = fixture().await;
    let outcome = fixture
        .write(7, RuleChange::Create(rule("answer", respond())))
        .await;
    assert_eq!(outcome, WriteOutcome::Stale);
    assert_eq!(fixture.flags().await, (0, 0, false));
}

#[tokio::test]
async fn reorder_must_name_every_rule_exactly_once() {
    let fixture = fixture().await;
    let first = written(
        fixture
            .write(0, RuleChange::Create(rule("a", counter())))
            .await,
    );
    written(
        fixture
            .write(1, RuleChange::Create(rule("b", counter())))
            .await,
    );
    let only_first = vec![first.rules[0].id];
    let outcome = fixture.write(2, RuleChange::Reorder(only_first)).await;
    assert_eq!(outcome, WriteOutcome::OrderMismatch);
    assert_eq!(fixture.flags().await, (2, 2, false));
}

#[tokio::test]
async fn reorder_applies_the_listed_order() {
    let fixture = fixture().await;
    let first = written(
        fixture
            .write(0, RuleChange::Create(rule("a", counter())))
            .await,
    );
    let second = written(
        fixture
            .write(1, RuleChange::Create(rule("b", counter())))
            .await,
    );
    let ids: Vec<Uuid> = vec![second.rules[1].id, first.rules[0].id];
    let set = written(fixture.write(2, RuleChange::Reorder(ids)).await);
    let names: Vec<&str> = set.rules.iter().map(|rule| rule.name.as_str()).collect();
    assert_eq!(names, ["b", "a"]);
}
