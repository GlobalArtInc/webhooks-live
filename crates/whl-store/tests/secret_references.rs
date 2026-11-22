use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::json;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_rules::{Action, Condition, Rule, Step};
use whl_store::notify::{ChannelKind, CreateChannelOutcome, NewChannel, Trigger};
use whl_store::rules::{RuleChange, WriteOutcome};
use whl_store::schedules::{CreateScheduleOutcome, NewSchedule, ScheduleRequest};
use whl_store::{EndpointSettings, Overflow, Store, Uuid, VerifiedIdentity};
use whl_testkit::TestDb;

const RETENTION: Duration = Duration::from_secs(3600);
const SECRET: &str = "HOOK_KEY";

struct Fixture {
    db: TestDb,
    store: Store,
    owner: Uuid,
    endpoint: EndpointId,
}

async fn fixture() -> Fixture {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let owner = store
        .upsert_user(&VerifiedIdentity {
            issuer: "https://sso.test".to_owned(),
            subject: "owner".to_owned(),
            email: "owner@example.test".to_owned(),
            email_verified: true,
            display_name: None,
        })
        .await
        .expect("the user is stored")
        .id;
    let endpoint = store
        .create_endpoint_owned(
            Some(owner),
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
        .expect("the endpoint is created")
        .id;
    Fixture {
        db,
        store,
        owner,
        endpoint,
    }
}

impl Fixture {
    async fn put_secret(&self) {
        sqlx::query(
            "INSERT INTO endpoint_secrets (endpoint_id, name, key_id, nonce, ciphertext) \
             VALUES ($1, $2, 1, $3, $4)",
        )
        .bind(self.endpoint.unpack())
        .bind(SECRET)
        .bind(vec![0u8; 24])
        .bind(vec![0u8; 32])
        .execute(self.db.pool())
        .await
        .expect("the secret is stored");
    }

    async fn drop_secret(&self) {
        sqlx::query("DELETE FROM endpoint_secrets WHERE endpoint_id = $1 AND name = $2")
            .bind(self.endpoint.unpack())
            .bind(SECRET)
            .execute(self.db.pool())
            .await
            .expect("the secret is deleted");
    }
}

fn required() -> BTreeSet<String> {
    BTreeSet::from([SECRET.to_owned()])
}

fn rule() -> Rule {
    Rule {
        name: "answer".to_owned(),
        enabled: true,
        condition: Condition::Always,
        steps: vec![Step {
            name: None,
            continue_on_error: false,
            action: Action::Respond {
                status: "200".to_owned(),
                content_type: None,
                headers: Vec::new(),
                body: String::new(),
            },
        }],
    }
}

fn new_schedule(owner: Uuid) -> NewSchedule {
    NewSchedule {
        created_by: owner,
        name: "probe".to_owned(),
        cron: "0 * * * *".to_owned(),
        timezone: "UTC".to_owned(),
        request: ScheduleRequest {
            method: "GET".to_owned(),
            url: "https://example.com/{{ secret.HOOK_KEY }}".to_owned(),
            headers: Vec::new(),
            body: None,
        },
        enabled: false,
        next_run_at: None,
    }
}

fn telegram_channel() -> NewChannel {
    NewChannel {
        id: Uuid::now_v7(),
        kind: ChannelKind::Telegram,
        name: "bot".to_owned(),
        enabled: true,
        trigger: Trigger::Capture,
        config: json!({ "token_secret": SECRET, "chat_id": "123" }),
        sealed: None,
        template: None,
        per_hour: 10,
        created_by: None,
    }
}

#[tokio::test]
async fn a_rule_write_is_refused_when_its_secret_was_deleted_after_validation() {
    let fixture = fixture().await;
    fixture.put_secret().await;
    let first = fixture
        .store
        .write_rules_requiring(
            &fixture.endpoint,
            RETENTION,
            0,
            &RuleChange::Create(rule()),
            &required(),
        )
        .await
        .expect("the write runs");
    assert!(matches!(first, WriteOutcome::Written(_)));

    fixture.drop_secret().await;
    let second = fixture
        .store
        .write_rules_requiring(
            &fixture.endpoint,
            RETENTION,
            1,
            &RuleChange::Create(rule()),
            &required(),
        )
        .await
        .expect("the write runs");

    assert_eq!(
        second,
        WriteOutcome::SecretsMissing(vec![SECRET.to_owned()])
    );
    let set = fixture
        .store
        .list_rules(&fixture.endpoint)
        .await
        .expect("the rules are listed");
    assert_eq!((set.version, set.rules.len()), (1, 1));
}

#[tokio::test]
async fn a_schedule_create_is_refused_when_its_secret_was_deleted_after_validation() {
    let fixture = fixture().await;
    fixture.put_secret().await;
    let schedule = new_schedule(fixture.owner);
    let first = fixture
        .store
        .create_schedule_requiring(&fixture.endpoint, RETENTION, &schedule, 10, &required())
        .await
        .expect("the create runs");
    assert!(matches!(first, CreateScheduleOutcome::Created(_)));

    fixture.drop_secret().await;
    let second = fixture
        .store
        .create_schedule_requiring(&fixture.endpoint, RETENTION, &schedule, 10, &required())
        .await
        .expect("the create runs");

    assert_eq!(
        second,
        CreateScheduleOutcome::SecretsMissing(vec![SECRET.to_owned()])
    );
    let held = fixture
        .store
        .list_schedules(&fixture.endpoint)
        .await
        .expect("the schedules are listed");
    assert_eq!(held.len(), 1);
}

#[tokio::test]
async fn a_telegram_channel_create_is_refused_when_its_secret_was_deleted_after_validation() {
    let fixture = fixture().await;
    fixture.put_secret().await;
    let first = fixture
        .store
        .create_channel(&fixture.endpoint, RETENTION, &telegram_channel())
        .await
        .expect("the create runs");
    assert!(matches!(first, CreateChannelOutcome::Created(_)));

    fixture.drop_secret().await;
    let second = fixture
        .store
        .create_channel(&fixture.endpoint, RETENTION, &telegram_channel())
        .await
        .expect("the create runs");

    assert_eq!(
        second,
        CreateChannelOutcome::SecretsMissing(vec![SECRET.to_owned()])
    );
    let held = fixture
        .store
        .list_channels(&fixture.endpoint)
        .await
        .expect("the channels are listed");
    assert_eq!(held.len(), 1);
}
