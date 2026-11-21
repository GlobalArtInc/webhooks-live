mod support;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use serde_json::{Value, json};
use support::{
    ChannelSpec, Device, Seen, StatusMap, StatusRoute, limits, loopback_guard, rig, rig_replying,
    rig_routed, wait_until,
};
use whl_notify::{
    PushHosts, PushService, Transport, Vapid, VapidError, VapidSubject, VendorHosts, decrypt,
    load_or_create_vapid,
};
use whl_secrets::Keyring;
use whl_store::notify::PushResults;

const FCM: &str = "https://fcm.googleapis.com";
const MOZILLA: &str = "https://updates.push.services.mozilla.com";

fn payload_of(request: &Seen, device: &Device) -> Value {
    let bytes = decrypt(&device.scalar, &device.auth, &request.body)
        .expect("the body decrypts with the device keys");
    serde_json::from_slice(&bytes).expect("the payload is JSON")
}

fn request_to<'a>(seen: &'a [Seen], target: &str) -> &'a Seen {
    seen.iter()
        .find(|request| request.target == target)
        .unwrap_or_else(|| panic!("no request reached {target}"))
}

fn assert_signed_by(service: &PushService, authorization: &str, audience: &str) {
    let rest = authorization
        .strip_prefix("vapid t=")
        .expect("the vapid scheme");
    let (token, key) = rest.split_once(", k=").expect("the key parameter");
    assert_eq!(key, service.public_key());
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let decode = |text: &str| URL_SAFE_NO_PAD.decode(text).expect("valid base64url");
    let claims: Value = serde_json::from_slice(&decode(parts[1])).expect("the claims are JSON");
    assert_eq!(claims["aud"], audience);
    assert_eq!(claims["sub"], "mailto:ops@hooks.example");
    let now = u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp()).expect("a date");
    let expiry = claims["exp"].as_u64().expect("an expiry");
    assert!(expiry > now && expiry <= now + 24 * 3600);
    let verifying = VerifyingKey::from_sec1_bytes(&decode(key)).expect("a public key");
    let signature = Signature::from_slice(&decode(parts[2])).expect("a raw signature");
    verifying
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
        .expect("the signature verifies under the published key");
}

fn gone_routes(default: u16, routes: &[(&str, u16)]) -> StatusMap {
    StatusMap {
        default,
        by_target: routes
            .iter()
            .map(|(target, status)| StatusRoute {
                target: (*target).to_owned(),
                status: *status,
            })
            .collect(),
    }
}

#[tokio::test]
async fn a_push_delivery_reaches_every_subscription_of_the_creator_with_an_encrypted_summary() {
    let rig = rig_replying(201).await;
    let creator = rig.user("creator").await;
    let stranger = rig.user("stranger").await;
    let first = rig.device(creator, &format!("{FCM}/fcm/send/first")).await;
    let second = rig
        .device(creator, &format!("{MOZILLA}/wpush/v2/second"))
        .await;
    rig.device(stranger, &format!("{FCM}/fcm/send/stranger"))
        .await;
    rig.channel(ChannelSpec::push(creator)).await;
    let capture = rig
        .capture("POST", "/orders/42", b"{\"customer\":\"private body\"}")
        .await;
    let service = rig.push_service();
    let running = rig.start_push(limits(30, 300, 60), service.clone());
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let seen = rig.server.seen();
    assert_eq!(seen.len(), 2);
    for (target, device, audience) in [
        ("/fcm/send/first", &first, FCM),
        ("/wpush/v2/second", &second, MOZILLA),
    ] {
        let request = request_to(&seen, target);
        assert_eq!(request.method, "POST");
        assert_eq!(request.header("content-encoding"), Some("aes128gcm"));
        assert_eq!(
            request.header("content-type"),
            Some("application/octet-stream")
        );
        assert_eq!(request.header("ttl"), Some("3600"));
        assert_signed_by(
            &service,
            request.header("authorization").expect("an authorization"),
            audience,
        );
        assert_eq!(
            payload_of(request, device),
            json!({
                "endpoint_id": rig.endpoint.unpack(),
                "capture_id": capture.to_string(),
                "kind": "http",
                "method": "POST",
                "path": "/orders/42",
            })
        );
    }
    assert!(
        !seen
            .iter()
            .any(|request| request.target == "/fcm/send/stranger")
    );
    let outcome = rig.delivery_outcome().await;
    assert_eq!(
        (outcome.state.as_str(), outcome.status),
        ("succeeded", Some(201))
    );
}

#[tokio::test]
async fn subscriptions_answered_with_404_or_410_are_removed_and_the_rest_keep_receiving() {
    let rig = rig_routed(gone_routes(
        201,
        &[("/fcm/send/gone404", 404), ("/fcm/send/gone410", 410)],
    ))
    .await;
    let creator = rig.user("creator").await;
    rig.device(creator, &format!("{FCM}/fcm/send/gone404"))
        .await;
    rig.device(creator, &format!("{FCM}/fcm/send/gone410"))
        .await;
    let alive = rig.device(creator, &format!("{FCM}/fcm/send/alive")).await;
    rig.channel(ChannelSpec::push(creator)).await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start_push(limits(30, 300, 60), rig.push_service());
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    assert_eq!(rig.server.seen().len(), 3);
    let remaining: Vec<whl_store::Uuid> = sqlx::query_scalar("SELECT id FROM push_subscriptions")
        .fetch_all(rig.db.pool())
        .await
        .expect("the subscriptions are readable");
    assert_eq!(remaining, [alive.subscription]);
}

#[tokio::test]
async fn a_delivery_whose_subscriptions_are_all_gone_fails_without_a_retry() {
    let rig = rig_replying(410).await;
    let creator = rig.user("creator").await;
    rig.device(creator, &format!("{FCM}/fcm/send/one")).await;
    rig.device(creator, &format!("{MOZILLA}/wpush/v2/two"))
        .await;
    rig.channel(ChannelSpec::push(creator)).await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start_push(limits(30, 300, 60), rig.push_service());
    wait_until("the delivery", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    let outcome = rig.delivery_outcome().await;
    assert_eq!(outcome.status, Some(410));
    assert_eq!(outcome.attempts, 1);
    assert!(
        outcome
            .error
            .as_deref()
            .is_some_and(|error| error.starts_with("subscription_gone")),
        "{outcome:?}"
    );
    assert_eq!(rig.subscription_count().await, 0);
}

async fn failures_of(rig: &support::Rig, subscription: whl_store::Uuid) -> i16 {
    sqlx::query_scalar("SELECT failures FROM push_subscriptions WHERE id = $1")
        .bind(subscription)
        .fetch_one(rig.db.pool())
        .await
        .expect("the subscription is readable")
}

#[tokio::test]
async fn a_subscription_refused_with_a_client_error_five_times_in_a_row_is_removed() {
    let rig = rig_replying(403).await;
    let creator = rig.user("creator").await;
    let device = rig
        .device(creator, &format!("{FCM}/fcm/send/refused"))
        .await;
    rig.channel(ChannelSpec::push(creator)).await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start_push(limits(30, 300, 60), rig.push_service());
    wait_until("the delivery", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;
    assert_eq!(failures_of(&rig, device.subscription).await, 1);

    let rejected = [device.subscription];
    let refuse = || PushResults {
        delivered: &[],
        rejected: &rejected,
    };
    for _ in 0..3 {
        assert_eq!(
            rig.store
                .record_push_results(refuse())
                .await
                .expect("recorded"),
            0
        );
    }
    assert_eq!(failures_of(&rig, device.subscription).await, 4);
    let accepted = PushResults {
        delivered: &rejected,
        rejected: &[],
    };
    rig.store
        .record_push_results(accepted)
        .await
        .expect("recorded");
    assert_eq!(failures_of(&rig, device.subscription).await, 0);

    for _ in 0..4 {
        assert_eq!(
            rig.store
                .record_push_results(refuse())
                .await
                .expect("recorded"),
            0
        );
    }
    assert_eq!(rig.subscription_count().await, 1);
    assert_eq!(
        rig.store
            .record_push_results(refuse())
            .await
            .expect("recorded"),
        1
    );
    assert_eq!(rig.subscription_count().await, 0);
}

#[tokio::test]
async fn a_stored_subscription_on_a_foreign_host_is_never_contacted() {
    let rig = rig().await;
    let creator = rig.user("creator").await;
    rig.device(creator, "https://evil.test/push/capability")
        .await;
    rig.device(
        creator,
        "https://fcm.googleapis.com.evil.test/push/capability",
    )
    .await;
    rig.channel(ChannelSpec::push(creator)).await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start_push(limits(30, 300, 60), rig.push_service());
    wait_until("the delivery", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(rig.server.seen().is_empty());
    let outcome = rig.delivery_outcome().await;
    let error = outcome.error.expect("an error is recorded");
    assert!(error.contains("not_pinned"), "{error}");
    assert!(!error.contains("capability"), "{error}");
    assert_eq!(rig.subscription_count().await, 2);
}

#[tokio::test]
async fn a_configured_extra_host_is_contacted_and_other_hosts_still_are_not() {
    let rig = rig_replying(201).await;
    let creator = rig.user("creator").await;
    rig.device(creator, "https://push.example.test/ok").await;
    rig.device(creator, "https://other.example.test/no").await;
    rig.channel(ChannelSpec::push(creator)).await;
    rig.capture("POST", "/a", b"").await;
    let hosts = PushHosts::vendors()
        .with_extras(&["push.example.test".to_owned()])
        .expect("a valid extra host");
    let transport = Transport::new(
        loopback_guard(),
        None,
        VendorHosts::production()
            .with_push_hosts(hosts.clone())
            .with_override("push.example.test", rig.server.origin()),
    )
    .expect("the transport is built");
    let subject = VapidSubject::parse("mailto:ops@hooks.example").expect("a subject");
    let service = PushService::new(Vapid::generate(subject).expect("a key pair"), hosts);
    let running = rig.start_push_with(transport, limits(30, 300, 60), service);
    wait_until("the delivery", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;

    let seen = rig.server.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].target, "/ok");
}

#[tokio::test]
async fn a_channel_whose_creator_has_no_subscriptions_sends_nothing() {
    let rig = rig().await;
    let creator = rig.user("creator").await;
    rig.channel(ChannelSpec::push(creator)).await;
    rig.capture("POST", "/a", b"").await;
    let running = rig.start_push(limits(30, 300, 60), rig.push_service());
    wait_until("the delivery", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    running.stop().await;

    assert!(rig.server.seen().is_empty());
    let error = rig.delivery_outcome().await.error.expect("an error");
    assert!(error.starts_with("no_subscriptions"), "{error}");
}

#[tokio::test]
async fn a_creator_who_cannot_view_the_endpoint_receives_nothing_until_made_a_member() {
    let rig = rig().await;
    let owner = rig.user("owner").await;
    let creator = rig.user("creator").await;
    let endpoint = rig.owned_endpoint(owner).await;
    rig.device(creator, &format!("{FCM}/fcm/send/dev")).await;
    rig.channel_on(&endpoint, ChannelSpec::push(creator)).await;
    rig.capture_on(&endpoint, "GET", "/x", b"").await;
    let running = rig.start_push(limits(30, 300, 60), rig.push_service());
    wait_until("the refused delivery", || async {
        rig.count_in_state("failed").await == 1
    })
    .await;
    assert!(rig.server.seen().is_empty());
    let error = rig.delivery_outcome().await.error.expect("an error");
    assert!(error.starts_with("creator_lost_access"), "{error}");

    sqlx::query(
        "INSERT INTO endpoint_members (endpoint_id, user_id, role) VALUES ($1, $2, 'viewer')",
    )
    .bind(endpoint.unpack())
    .bind(creator)
    .execute(rig.db.pool())
    .await
    .expect("the member is added");
    rig.capture_on(&endpoint, "GET", "/y", b"").await;
    wait_until("the delivery to the member", || async {
        rig.count_in_state("succeeded").await == 1
    })
    .await;
    running.stop().await;
    assert_eq!(rig.server.seen().len(), 1);
}

#[tokio::test]
async fn push_deliveries_are_limited_by_the_channel_counter_like_every_other_kind() {
    let rig = rig_replying(201).await;
    let creator = rig.user("creator").await;
    rig.device(creator, &format!("{FCM}/fcm/send/one")).await;
    let mut spec = ChannelSpec::push(creator);
    spec.per_hour = 1;
    rig.channel(spec).await;
    for path in ["/1", "/2"] {
        rig.capture("POST", path, b"").await;
    }
    let running = rig.start_push(limits(30, 300, 60), rig.push_service());
    wait_until("one sent and one suppressed", || async {
        rig.count_in_state("succeeded").await == 1 && rig.count_in_state("suppressed").await == 1
    })
    .await;
    running.stop().await;

    assert_eq!(rig.server.seen().len(), 1);
}

#[tokio::test]
async fn the_vapid_key_is_created_once_stored_sealed_and_opens_only_with_the_same_keys() {
    let rig = rig().await;
    let subject = VapidSubject::parse("mailto:ops@hooks.example").expect("a subject");
    let (left, right) = tokio::join!(
        load_or_create_vapid(&rig.store, &rig.keyring, &subject),
        load_or_create_vapid(&rig.store, &rig.keyring, &subject)
    );
    let (left, right) = (left.expect("a key"), right.expect("a key"));
    assert_eq!(left.public_bytes(), right.public_bytes());
    let again = load_or_create_vapid(&rig.store, &rig.keyring, &subject)
        .await
        .expect("the stored key loads");
    assert_eq!(again.public_bytes(), left.public_bytes());
    assert_eq!(
        left.public_key(),
        URL_SAFE_NO_PAD.encode(left.public_bytes())
    );

    let rows: Vec<(Vec<u8>, Vec<u8>)> =
        sqlx::query_as("SELECT public_key, secret_ciphertext FROM push_vapid_keys")
            .fetch_all(rig.db.pool())
            .await
            .expect("the rows are readable");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, left.public_bytes().to_vec());
    assert_eq!(rows[0].1.len(), 43 + 16);

    let other = Keyring::parse(&format!("1:{}", STANDARD.encode([2_u8; 32]))).expect("a key set");
    assert!(matches!(
        load_or_create_vapid(&rig.store, &other, &subject).await,
        Err(VapidError::Open(_))
    ));
}
