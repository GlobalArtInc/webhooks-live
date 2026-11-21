use std::process::{Command, Output};

use figment::Jail;
use whl_core::config::Config;
use whl_core::endpoint_id::EndpointId;
use whl_core::response::ResponseConfig;
use whl_secrets::{KeyId, Keyring, Secret, SecretName};
use whl_server::{RotateError, RotateReport, rotate_secrets};
use whl_store::secrets::{PutSecretOutcome, ReplaceSealedOutcome, RotationCursor, StoredSecret};
use whl_store::{EndpointSettings, Overflow, Store};
use whl_testkit::TestDb;

const KEY_ONE: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const KEY_TWO: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";
const RETENTION: std::time::Duration = std::time::Duration::from_secs(3600);

fn load(vars: &[(&str, &str)]) -> Config {
    let mut result = None;
    #[allow(clippy::result_large_err)]
    Jail::expect_with(|jail| {
        let base = [
            ("WEBHOOKS_LIVE_PUBLIC_URL", "https://whl.test"),
            (
                "WEBHOOKS_LIVE_DATABASE_URL",
                "postgres://u:p@127.0.0.1:1/none",
            ),
            ("WEBHOOKS_LIVE_LISTEN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_ADMIN_ADDR", "127.0.0.1:0"),
            ("WEBHOOKS_LIVE_OIDC_ISSUER", "https://issuer.test"),
            ("WEBHOOKS_LIVE_OIDC_CLIENT_ID", "client"),
            ("WEBHOOKS_LIVE_OIDC_CLIENT_SECRET", "secret"),
        ];
        for (key, value) in base.iter().chain(vars.iter()) {
            jail.set_env(key, value);
        }
        result = Some(Config::load());
        Ok(())
    });
    result
        .expect("the jail closure always runs")
        .expect("the configuration loads")
}

fn ring(spec: &str) -> Keyring {
    Keyring::parse(spec).expect("a valid key set")
}

async fn bind_error(vars: &[(&str, &str)]) -> String {
    match whl_server::bind(&load(vars)).await {
        Ok(_) => panic!("binding must be refused"),
        Err(error) => error.to_string(),
    }
}

#[tokio::test]
async fn sso_and_mixed_refuse_to_start_without_a_key_set() {
    for mode in ["sso", "mixed"] {
        let text = bind_error(&[("WEBHOOKS_LIVE_AUTH_MODE", mode)]).await;

        assert!(
            text.contains("WEBHOOKS_LIVE_SECRETS_KEYS is required"),
            "{text}"
        );
        assert!(text.contains(mode), "{text}");
    }
    let blank = bind_error(&[
        ("WEBHOOKS_LIVE_AUTH_MODE", "sso"),
        ("WEBHOOKS_LIVE_SECRETS_KEYS", "  "),
    ])
    .await;
    assert!(blank.contains("is required"), "{blank}");
}

#[tokio::test]
async fn a_malformed_key_set_is_refused_in_every_mode_without_echoing_it() {
    let leaked = "1:QUJD-not-valid-base64-but-secret-looking";
    for mode in ["open", "sso"] {
        let text = bind_error(&[
            ("WEBHOOKS_LIVE_AUTH_MODE", mode),
            ("WEBHOOKS_LIVE_SECRETS_KEYS", leaked),
        ])
        .await;

        assert!(
            text.contains("WEBHOOKS_LIVE_SECRETS_KEYS is invalid"),
            "{text}"
        );
        assert!(text.contains("entry 1"), "{text}");
        assert!(!text.contains("QUJD"), "{text}");
    }
    let short = bind_error(&[("WEBHOOKS_LIVE_SECRETS_KEYS", "1:AAAA")]).await;
    assert!(short.contains("32 bytes"), "{short}");
}

#[tokio::test]
async fn open_mode_without_a_key_set_starts_with_the_feature_disabled() {
    let config = load(&[]);

    assert!(
        whl_server::keyring_from_config(&config)
            .expect("open mode needs no keys")
            .is_none()
    );
    whl_server::bind(&config).await.expect("binds");

    let keyed = load(&[(
        "WEBHOOKS_LIVE_SECRETS_KEYS",
        &format!("2:{KEY_TWO},1:{KEY_ONE}"),
    )]);
    let keyring = whl_server::keyring_from_config(&keyed)
        .expect("valid")
        .expect("enabled");
    assert_eq!(keyring.active_id().unpack(), 2);
    assert!(format!("{keyed:?}").contains("SecretKeys(<redacted>)"));
    assert!(!format!("{keyed:?}").contains(KEY_TWO));
}

async fn seed(store: &Store, keyring: &Keyring, rows: &[(&EndpointId, &str, &str)]) {
    for (endpoint, name, value) in rows {
        let name = SecretName::parse(name).expect("a name");
        let value = Secret::new((*value).to_owned()).expect("a value");
        let sealed = keyring.seal(endpoint, &name, &value).expect("seals");
        let outcome = store
            .put_secret(endpoint, RETENTION, &name, &sealed, 20)
            .await
            .expect("stores");
        assert!(matches!(outcome, PutSecretOutcome::Stored(_)));
    }
}

async fn endpoint(store: &Store) -> EndpointId {
    store
        .create_endpoint_from_network(
            None,
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
        .expect("the endpoint is stored")
        .id
}

async fn key_ids(db: &TestDb) -> Vec<i16> {
    sqlx::query_scalar("SELECT key_id FROM endpoint_secrets ORDER BY endpoint_id, name")
        .fetch_all(db.pool())
        .await
        .expect("reads")
}

async fn run(store: &Store, keyring: &Keyring, batch: u32) -> (RotateReport, usize) {
    let mut calls = 0;
    let report = rotate_secrets(store, keyring, batch, |_| calls += 1)
        .await
        .expect("rotates");
    (report, calls)
}

#[tokio::test]
async fn rotation_re_seals_every_row_in_batches_and_a_second_run_changes_nothing() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let (first, second) = (endpoint(&store).await, endpoint(&store).await);
    let old = ring(&format!("1:{KEY_ONE}"));
    let rotated = ring(&format!("2:{KEY_TWO},1:{KEY_ONE}"));
    let rows = [
        (&first, "A_ONE", "value-one"),
        (&first, "B_TWO", "value-two"),
        (&first, "C_THREE", "value-three"),
        (&second, "A_ONE", "value-four"),
        (&second, "B_TWO", "value-five"),
        (&second, "C_THREE", "value-six"),
        (&second, "D_FOUR", "value-seven"),
    ];
    seed(&store, &old, &rows).await;
    assert_eq!(key_ids(&db).await, [1; 7]);
    for (endpoint, name, value) in &rows {
        let sealed = store
            .get_sealed_secret(endpoint, &SecretName::parse(name).expect("name"))
            .await
            .expect("reads")
            .expect("exists");
        let opened = rotated
            .open(endpoint, &SecretName::parse(name).expect("name"), &sealed)
            .expect("the retained key reads before rotation");
        assert_eq!(opened.unpack(), *value);
    }

    let (report, calls) = run(&store, &rotated, 3).await;

    assert_eq!(
        report,
        RotateReport {
            batches: 3,
            rotated: 7,
            skipped: 0,
            failed: 0
        }
    );
    assert_eq!(calls, 3);
    assert_eq!(key_ids(&db).await, [2; 7]);
    let only_new = ring(&format!("2:{KEY_TWO}"));
    for (endpoint, name, value) in &rows {
        let name = SecretName::parse(name).expect("name");
        let sealed = store
            .get_sealed_secret(endpoint, &name)
            .await
            .expect("reads")
            .expect("exists");
        assert_eq!(
            only_new
                .open(endpoint, &name, &sealed)
                .expect("opens")
                .unpack(),
            *value
        );
    }

    let (again, calls) = run(&store, &rotated, 3).await;

    assert_eq!(again, RotateReport::default());
    assert_eq!(calls, 0);
}

#[tokio::test]
async fn an_interrupted_rotation_resumes_with_only_the_remaining_rows() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let target = endpoint(&store).await;
    let old = ring(&format!("1:{KEY_ONE}"));
    let rotated = ring(&format!("2:{KEY_TWO},1:{KEY_ONE}"));
    seed(
        &store,
        &old,
        &[
            (&target, "A_ONE", "value-one"),
            (&target, "B_TWO", "value-two"),
            (&target, "C_THREE", "value-three"),
        ],
    )
    .await;
    let done = store
        .rotation_batch(rotated.active_id(), &RotationCursor::start(), 1)
        .await
        .expect("reads");
    let stored = done[0].stored.clone().expect("a well formed row");
    let replacement = rotated
        .reseal(&stored.endpoint, &stored.name, &stored.sealed)
        .expect("reseals");
    assert_eq!(
        store
            .replace_sealed_secret(&stored, &replacement)
            .await
            .expect("replaces"),
        ReplaceSealedOutcome::Replaced
    );

    let (report, _) = run(&store, &rotated, 100).await;

    assert_eq!(report.rotated, 2);
    assert_eq!(key_ids(&db).await, [2, 2, 2]);
}

#[tokio::test]
async fn a_row_that_cannot_be_opened_is_reported_and_left_untouched() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let (first, second) = (endpoint(&store).await, endpoint(&store).await);
    let old = ring(&format!("1:{KEY_ONE}"));
    let lost = ring(&format!("9:{KEY_ONE}"));
    let rotated = ring(&format!("2:{KEY_TWO},1:{KEY_ONE}"));
    seed(
        &store,
        &old,
        &[
            (&first, "A_ONE", "value-one"),
            (&first, "B_TWO", "value-two"),
        ],
    )
    .await;
    seed(&store, &lost, &[(&second, "A_ONE", "value-three")]).await;
    sqlx::query(
        "UPDATE endpoint_secrets SET nonce = o.nonce, ciphertext = o.ciphertext \
         FROM endpoint_secrets o \
         WHERE endpoint_secrets.endpoint_id = $1 AND endpoint_secrets.name = 'B_TWO' \
         AND o.endpoint_id = $1 AND o.name = 'A_ONE'",
    )
    .bind(first.unpack())
    .execute(db.pool())
    .await
    .expect("rows are swapped");

    let (report, _) = run(&store, &rotated, 100).await;

    assert_eq!(
        report.rotated, 1,
        "only the untouched A_ONE of the first endpoint"
    );
    assert_eq!(
        report.failed, 2,
        "the swapped row and the row under the lost key"
    );
    let mut remaining = key_ids(&db).await;
    remaining.sort_unstable();
    assert_eq!(remaining, [1, 2, 9]);
    assert_eq!(
        store
            .count_secrets_outside_key(KeyId::new(2).expect("id"))
            .await
            .expect("counts"),
        2
    );
}

#[tokio::test]
async fn a_row_rewritten_while_rotation_runs_is_not_overwritten_with_stale_data() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let target = endpoint(&store).await;
    let old = ring(&format!("1:{KEY_ONE}"));
    let rotated = ring(&format!("2:{KEY_TWO},1:{KEY_ONE}"));
    seed(&store, &old, &[(&target, "A_ONE", "old-value")]).await;
    let batch = store
        .rotation_batch(rotated.active_id(), &RotationCursor::start(), 10)
        .await
        .expect("reads");
    let stale: StoredSecret = batch[0].stored.clone().expect("a well formed row");
    let replacement = rotated
        .reseal(&stale.endpoint, &stale.name, &stale.sealed)
        .expect("reseals");
    seed(&store, &rotated, &[(&target, "A_ONE", "newer-value")]).await;

    let outcome = store
        .replace_sealed_secret(&stale, &replacement)
        .await
        .expect("runs");

    assert_eq!(outcome, ReplaceSealedOutcome::Changed);
    let current = store
        .get_sealed_secret(&target, &stale.name)
        .await
        .expect("reads")
        .expect("exists");
    assert_eq!(
        rotated
            .open(&target, &stale.name, &current)
            .expect("opens")
            .unpack(),
        "newer-value"
    );
}

#[tokio::test]
async fn the_batch_size_is_bounded() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let keyring = ring(&format!("1:{KEY_ONE}"));

    for size in [0, whl_server::MAX_BATCH_SIZE + 1] {
        let error = rotate_secrets(&store, &keyring, size, |_| {})
            .await
            .expect_err("out of range");
        assert!(matches!(error, RotateError::BatchSize));
    }
}

fn command(db: &TestDb, extra: &[(&str, &str)], args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_webhooks-live"));
    command
        .env_clear()
        .env("WEBHOOKS_LIVE_PUBLIC_URL", "https://whl.test")
        .env("WEBHOOKS_LIVE_DATABASE_URL", db.url())
        .env("WEBHOOKS_LIVE_DB_POOL_SIZE", "3")
        .args(args);
    for (key, value) in extra {
        command.env(key, value);
    }
    command.output().expect("the binary runs")
}

#[tokio::test]
async fn the_rotate_subcommand_reports_progress_and_exits_with_the_outcome() {
    let db = TestDb::new().await;
    let store = Store::from_pool(db.pool().clone());
    let target = endpoint(&store).await;
    let old = ring(&format!("1:{KEY_ONE}"));
    seed(
        &store,
        &old,
        &[
            (&target, "A_ONE", "value-one"),
            (&target, "B_TWO", "value-two"),
            (&target, "C_THREE", "value-three"),
        ],
    )
    .await;
    let keys = format!("2:{KEY_TWO},1:{KEY_ONE}");

    let missing = command(&db, &[], &["secrets", "rotate"]);
    let first = command(
        &db,
        &[("WEBHOOKS_LIVE_SECRETS_KEYS", &keys)],
        &["secrets", "rotate", "--batch-size", "2"],
    );
    let second = command(
        &db,
        &[("WEBHOOKS_LIVE_SECRETS_KEYS", &keys)],
        &["secrets", "rotate"],
    );
    let invalid = command(
        &db,
        &[("WEBHOOKS_LIVE_SECRETS_KEYS", &keys)],
        &["secrets", "rotate", "--batch-size", "0"],
    );

    assert_eq!(missing.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("WEBHOOKS_LIVE_SECRETS_KEYS is required")
    );
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let stdout = String::from_utf8_lossy(&first.stdout);
    assert!(stdout.contains("batch 1: rotated 2 of 3"), "{stdout}");
    assert!(stdout.contains("batch 2: rotated 3 of 3"), "{stdout}");
    assert!(stdout.contains("done: rotated 3"), "{stdout}");
    assert!(!stdout.contains(KEY_TWO) && !String::from_utf8_lossy(&first.stderr).contains(KEY_TWO));
    assert_eq!(key_ids(&db).await, [2, 2, 2]);
    assert_eq!(second.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&second.stdout).contains("done: rotated 0"));
    assert_ne!(invalid.status.code(), Some(0));
}
