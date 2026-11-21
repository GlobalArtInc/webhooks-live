mod common;
mod served;
mod spy;

use std::time::Duration;

use axum::body::Body;
use common::{Harness, request};
use served::{Configured, endpoint_configured, make_templated};
use spy::Spy;
use whl_secrets::{Keyring, Secret, SecretName};

const KEYS: &str = "1:AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const SECRET: &str = "sk_live_51Hq-super-secret-value";
const RETENTION: Duration = Duration::from_secs(7 * 24 * 3600);

#[tokio::test]
async fn a_stored_secret_is_never_loaded_rendered_recorded_or_logged() {
    let harness = Harness::new().await;
    let id = endpoint_configured(
        &harness,
        Configured {
            body: b"token={{ secret.TOKEN }}".to_vec(),
            headers: vec![("X-Token", "{{ secret.TOKEN }}")],
            ..Configured::default()
        },
    )
    .await;
    make_templated(&harness, &id, None).await;
    let keyring = Keyring::parse(KEYS).expect("the key set is valid");
    let name = SecretName::parse("TOKEN").expect("a valid name");
    let value = Secret::new(SECRET.to_owned()).expect("a valid value");
    let sealed = keyring.seal(&id, &name, &value).expect("the secret seals");
    let store = whl_store::Store::from_pool(harness.db.pool().clone());
    store
        .put_secret(&id, RETENTION, &name, &sealed, 20)
        .await
        .expect("the secret is stored");
    let (spy, _guard) = Spy::install();

    let reply = harness
        .send(
            request("POST", &format!("/{id}?leak={SECRET}"))
                .header("x-leak", SECRET)
                .body(Body::from(SECRET))
                .expect("request builds"),
        )
        .await;

    assert_eq!(reply.header("x-webhooks-live-template"), Some("error"));
    assert_eq!(
        reply.header("x-webhooks-live-template-error"),
        Some("invalid")
    );
    assert!(!reply.body.contains(SECRET));
    assert!(!format!("{:?}", reply.headers).contains(SECRET));
    let meta = harness.wait_for_meta(&id, 1).await;
    assert!(!meta.to_string().contains(SECRET));
    assert!(!spy.everything().contains(SECRET));
    assert!(!spy.everything().contains("TOKEN="));
}
