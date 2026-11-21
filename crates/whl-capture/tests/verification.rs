mod common;

use axum::body::Body;
use common::{Harness, request};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use whl_capture::{INLINE_VERIFY_BYTES, router};
use whl_core::endpoint_id::EndpointId;
use whl_secrets::{Keyring, Secret, SecretName};
use whl_verify::{Verifier, VerifierInput};

const KEYS: &str = "1:QUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUE=";
const SECRET_NAME: &str = "HOOK_SECRET";
const SECRET_VALUE: &str = "It's a Secret to Everybody";
const GITHUB_BODY: &str = "Hello, World!";
const GITHUB_SIGNATURE: &str =
    "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";

async fn harness() -> Harness {
    let mut harness = Harness::new().await;
    let keyring = Keyring::parse(KEYS).expect("keyring parses");
    harness.state = harness.state.clone().with_secrets(keyring.into());
    harness.router = router(harness.state.clone());
    harness
}

async fn endpoint_with_github_verifier(harness: &Harness) -> EndpointId {
    let id = harness.endpoint().await;
    let keyring = Keyring::parse(KEYS).expect("keyring parses");
    let name = SecretName::parse(SECRET_NAME).expect("name");
    let secret = Secret::new(SECRET_VALUE.to_owned()).expect("secret");
    let sealed = keyring.seal(&id, &name, &secret).expect("seals");
    sqlx::query(
        "INSERT INTO endpoint_secrets (endpoint_id, name, key_id, nonce, ciphertext) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id.unpack())
    .bind(name.unpack())
    .bind(sealed.key_id().to_column())
    .bind(sealed.nonce().as_slice())
    .bind(sealed.ciphertext())
    .execute(harness.db.pool())
    .await
    .expect("secret inserts");
    let verifier = Verifier::new(&VerifierInput {
        scheme: "github".to_owned(),
        secret_name: SECRET_NAME.to_owned(),
        ..VerifierInput::default()
    })
    .expect("verifier");
    sqlx::query("UPDATE endpoints SET verifier = $2::text::jsonb WHERE id = $1")
        .bind(id.unpack())
        .bind(serde_json::to_string(&verifier.document()).expect("document"))
        .execute(harness.db.pool())
        .await
        .expect("verifier sets");
    id
}

async fn send_signed(
    harness: &Harness,
    id: &EndpointId,
    signature: Option<&str>,
    body: Vec<u8>,
) -> u16 {
    let mut builder = request("POST", &format!("/{id}"));
    if let Some(signature) = signature {
        builder = builder.header("x-hub-signature-256", signature);
    }
    harness
        .send(builder.body(Body::from(body)).expect("request builds"))
        .await
        .status
}

async fn verification_of(harness: &Harness, id: &EndpointId) -> Option<String> {
    sqlx::query_scalar("SELECT verification FROM captures WHERE endpoint_id = $1")
        .bind(id.unpack())
        .fetch_one(harness.db.pool())
        .await
        .expect("verification reads")
}

fn sign(body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(SECRET_VALUE.as_bytes()).expect("key");
    mac.update(body);
    let tag = mac.finalize().into_bytes();
    let hex: String = tag.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256={hex}")
}

#[tokio::test]
async fn correctly_signed_capture_is_valid() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    let status = send_signed(
        &harness,
        &id,
        Some(GITHUB_SIGNATURE),
        GITHUB_BODY.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &id).await.as_deref(),
        Some("valid")
    );
}

#[tokio::test]
async fn tampered_body_is_stored_as_invalid() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    let status = send_signed(
        &harness,
        &id,
        Some(GITHUB_SIGNATURE),
        b"Hello, World?".to_vec(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &id).await.as_deref(),
        Some("invalid")
    );
}

#[tokio::test]
async fn unsigned_capture_is_stored_as_missing() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    let status = send_signed(&harness, &id, None, GITHUB_BODY.as_bytes().to_vec()).await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &id).await.as_deref(),
        Some("missing")
    );
}

#[tokio::test]
async fn endpoint_without_a_verifier_stores_no_verification() {
    let harness = harness().await;
    let id = harness.endpoint().await;
    let status = send_signed(
        &harness,
        &id,
        Some(GITHUB_SIGNATURE),
        GITHUB_BODY.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(verification_of(&harness, &id).await, None);
}

#[tokio::test]
async fn body_above_the_inline_limit_is_verified_off_the_runtime_thread() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    let body = vec![b'x'; INLINE_VERIFY_BYTES + 1];
    let status = send_signed(&harness, &id, Some(&sign(&body)), body).await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &id).await.as_deref(),
        Some("valid")
    );
}

#[tokio::test]
async fn vanished_secret_still_stores_the_capture_as_invalid() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    sqlx::query("DELETE FROM endpoint_secrets WHERE endpoint_id = $1")
        .bind(id.unpack())
        .execute(harness.db.pool())
        .await
        .expect("secret deletes");
    let status = send_signed(
        &harness,
        &id,
        Some(GITHUB_SIGNATURE),
        GITHUB_BODY.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &id).await.as_deref(),
        Some("invalid")
    );
}

#[tokio::test]
async fn secret_sealed_for_another_endpoint_does_not_open() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    let other = harness.endpoint().await;
    sqlx::query("UPDATE endpoint_secrets SET endpoint_id = $2 WHERE endpoint_id = $1")
        .bind(id.unpack())
        .bind(other.unpack())
        .execute(harness.db.pool())
        .await
        .expect("secret moves");
    sqlx::query("UPDATE endpoints SET verifier = (SELECT verifier FROM endpoints WHERE id = $1) WHERE id = $2")
        .bind(id.unpack())
        .bind(other.unpack())
        .execute(harness.db.pool())
        .await
        .expect("verifier copies");
    let status = send_signed(
        &harness,
        &other,
        Some(GITHUB_SIGNATURE),
        GITHUB_BODY.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &other).await.as_deref(),
        Some("invalid")
    );
}

#[tokio::test]
async fn unreadable_stored_verifier_stores_the_capture_as_invalid() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    sqlx::query(
        "UPDATE endpoints SET verifier = jsonb_set(verifier, '{extra}', '1') WHERE id = $1",
    )
    .bind(id.unpack())
    .execute(harness.db.pool())
    .await
    .expect("verifier rewrites");
    let status = send_signed(
        &harness,
        &id,
        Some(GITHUB_SIGNATURE),
        GITHUB_BODY.as_bytes().to_vec(),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &id).await.as_deref(),
        Some("invalid")
    );
}

#[tokio::test]
async fn unsigned_capture_is_missing_even_when_the_secret_is_gone() {
    let harness = harness().await;
    let id = endpoint_with_github_verifier(&harness).await;
    sqlx::query("DELETE FROM endpoint_secrets WHERE endpoint_id = $1")
        .bind(id.unpack())
        .execute(harness.db.pool())
        .await
        .expect("secret deletes");
    let status = send_signed(&harness, &id, None, GITHUB_BODY.as_bytes().to_vec()).await;
    assert_eq!(status, 200);
    assert_eq!(
        verification_of(&harness, &id).await.as_deref(),
        Some("missing")
    );
}
