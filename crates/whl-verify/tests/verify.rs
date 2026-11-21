use http::{HeaderMap, HeaderName, HeaderValue};
use time::OffsetDateTime;
use whl_secrets::Secret;
use whl_verify::{
    ConfigProblem, Reason, SignedRequest, Verdict, Verification, Verifier, VerifierDocument,
    VerifierInput,
};

const GITHUB_SECRET: &str = "It's a Secret to Everybody";
const GITHUB_BODY: &str = "Hello, World!";
const GITHUB_SIGNATURE: &str =
    "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17";

const SHOPIFY_SECRET: &str = "hush";
const SHOPIFY_BODY: &str = r#"{"id":820982911946154508}"#;
const SHOPIFY_SIGNATURE: &str = "qBvrsZF7RfB1iS6BX6IHqJTp5L911P7intnPeTPsH0I=";

const STRIPE_SECRET: &str = "whsec_stripe_test";
const STRIPE_BODY: &str = r#"{"id":"evt_1","object":"event"}"#;
const STRIPE_TIMESTAMP: i64 = 1_700_000_000;
const STRIPE_SIGNATURE: &str = "a898e3d79c323403a1fae570a6a85c94a49f50d22b8ebc757a021998864a58f3";

const SLACK_SECRET: &str = "8f742231b10e8888abcd99yyyzzz85a5";
const SLACK_TIMESTAMP: i64 = 1_531_420_618;
const SLACK_BODY: &str = "token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&team_domain=testteamnow&channel_id=G8PSS9T3V&channel_name=foobar&user_id=U2CERLKJA&user_name=roadrunner&command=%2Fwebhook-collect&text=&response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands%2FT1DC2JH3J%2F397700885554%2F96rGlfmibIGlgcZRskXaIFfN&trigger_id=398738663015.47445629121.803a0bc887a14d10d2c447fce8b6703c";
const SLACK_SIGNATURE: &str = "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b503";

const STANDARD_SECRET: &str = "whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw";
const STANDARD_ID: &str = "msg_p5jXN8AQM9LWM0D4loKWxJek";
const STANDARD_TIMESTAMP: i64 = 1_614_265_330;
const STANDARD_BODY: &str = r#"{"test": 2432232314}"#;
const STANDARD_SIGNATURE: &str = "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE=";

const SHA1_SIGNATURE: &str = "d205924b98446f173f55bcf5dd2f25c196d1ef45";
const SHA512_SIGNATURE: &str =
    "ONgAMMuOVTNMQpA62l7u/Y0pu+0MEpBTBm6xZVCHEJllUp2b64fdCYIf0/d3j/NNSlEoux+cPN6tBmctO81q5A==";
const SHA256_SIGNATURE: &str = "dcc85dc068d612072f03b5fff1b7f93e7056aefe534e95f6ae6c5c6aa85715e0";
const GENERIC_TIMESTAMP: i64 = 1_700_000_000;

fn input(scheme: &str) -> VerifierInput {
    VerifierInput {
        scheme: scheme.to_owned(),
        secret_name: "WEBHOOK_SECRET".to_owned(),
        ..VerifierInput::default()
    }
}

fn generic(
    header: &str,
    algorithm: &str,
    encoding: &str,
    prefix: Option<&str>,
    timestamp: Option<&str>,
) -> Verifier {
    let mut spec = input("hmac");
    spec.header = Some(header.to_owned());
    spec.algorithm = Some(algorithm.to_owned());
    spec.encoding = Some(encoding.to_owned());
    spec.prefix = prefix.map(str::to_owned);
    spec.timestamp_header = timestamp.map(str::to_owned);
    Verifier::new(&spec).expect("valid generic verifier")
}

fn preset(scheme: &str) -> Verifier {
    Verifier::new(&input(scheme)).expect("valid preset")
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            HeaderName::from_bytes(name.as_bytes()).expect("name"),
            HeaderValue::from_str(value).expect("value"),
        );
    }
    map
}

fn at(seconds: i64) -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(seconds).expect("timestamp")
}

fn run(
    verifier: &Verifier,
    secret: &str,
    headers: &HeaderMap,
    body: &str,
    received_at: i64,
) -> Verdict {
    let secret = Secret::new(secret.to_owned()).expect("secret");
    verifier.verify(
        &secret,
        &SignedRequest {
            headers,
            body: body.as_bytes(),
            received_at: at(received_at),
        },
    )
}

fn expect(verdict: Verdict, verification: Verification, reason: Reason) {
    assert_eq!(
        (verdict.verification, verdict.reason),
        (verification, reason)
    );
}

fn github_headers() -> HeaderMap {
    headers(&[("x-hub-signature-256", GITHUB_SIGNATURE)])
}

fn stripe_headers(timestamp: i64, signatures: &str) -> HeaderMap {
    headers(&[("stripe-signature", &format!("t={timestamp},{signatures}"))])
}

fn slack_headers(timestamp: &str, signature: &str) -> HeaderMap {
    headers(&[
        ("x-slack-signature", signature),
        ("x-slack-request-timestamp", timestamp),
    ])
}

fn standard_headers(id: &str, timestamp: &str, signature: &str) -> HeaderMap {
    headers(&[
        ("webhook-id", id),
        ("webhook-timestamp", timestamp),
        ("webhook-signature", signature),
    ])
}

#[test]
fn github_published_vector_is_valid() {
    let verdict = run(
        &preset("github"),
        GITHUB_SECRET,
        &github_headers(),
        GITHUB_BODY,
        0,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn tampered_body_is_invalid() {
    let verdict = run(
        &preset("github"),
        GITHUB_SECRET,
        &github_headers(),
        "Hello, World?",
        0,
    );
    expect(verdict, Verification::Invalid, Reason::Mismatch);
}

#[test]
fn wrong_secret_is_invalid() {
    let verdict = run(
        &preset("github"),
        "another secret",
        &github_headers(),
        GITHUB_BODY,
        0,
    );
    expect(verdict, Verification::Invalid, Reason::Mismatch);
}

#[test]
fn absent_signature_header_is_missing() {
    let verdict = run(
        &preset("github"),
        GITHUB_SECRET,
        &HeaderMap::new(),
        GITHUB_BODY,
        0,
    );
    expect(verdict, Verification::Missing, Reason::SignatureAbsent);
}

#[test]
fn signature_without_the_prefix_is_invalid() {
    let bare = GITHUB_SIGNATURE.trim_start_matches("sha256=");
    let verdict = run(
        &preset("github"),
        GITHUB_SECRET,
        &headers(&[("x-hub-signature-256", bare)]),
        GITHUB_BODY,
        0,
    );
    expect(verdict, Verification::Invalid, Reason::SignatureMalformed);
}

#[test]
fn signature_that_is_not_hex_is_invalid() {
    let verdict = run(
        &preset("github"),
        GITHUB_SECRET,
        &headers(&[("x-hub-signature-256", "sha256=zz")]),
        GITHUB_BODY,
        0,
    );
    expect(verdict, Verification::Invalid, Reason::SignatureMalformed);
}

#[test]
fn signature_of_another_algorithm_is_not_accepted() {
    let sha1 = format!("sha256={SHA1_SIGNATURE}");
    let verdict = run(
        &preset("github"),
        "k1",
        &headers(&[("x-hub-signature-256", &sha1)]),
        "payload",
        0,
    );
    expect(verdict, Verification::Invalid, Reason::SignatureMalformed);
}

#[test]
fn shopify_vector_is_valid() {
    let verdict = run(
        &preset("shopify"),
        SHOPIFY_SECRET,
        &headers(&[("x-shopify-hmac-sha256", SHOPIFY_SIGNATURE)]),
        SHOPIFY_BODY,
        0,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn stripe_vector_is_valid_inside_the_window() {
    let verdict = run(
        &preset("stripe"),
        STRIPE_SECRET,
        &stripe_headers(STRIPE_TIMESTAMP, &format!("v1={STRIPE_SIGNATURE}")),
        STRIPE_BODY,
        STRIPE_TIMESTAMP + 300,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn stripe_second_signature_may_match_and_unknown_versions_are_ignored() {
    let list = format!("v0=00,v1={},v1={STRIPE_SIGNATURE}", "00".repeat(32));
    let verdict = run(
        &preset("stripe"),
        STRIPE_SECRET,
        &stripe_headers(STRIPE_TIMESTAMP, &list),
        STRIPE_BODY,
        STRIPE_TIMESTAMP,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn stripe_stale_timestamp_is_invalid() {
    let verdict = run(
        &preset("stripe"),
        STRIPE_SECRET,
        &stripe_headers(STRIPE_TIMESTAMP, &format!("v1={STRIPE_SIGNATURE}")),
        STRIPE_BODY,
        STRIPE_TIMESTAMP + 301,
    );
    expect(
        verdict,
        Verification::Invalid,
        Reason::TimestampOutOfTolerance,
    );
}

#[test]
fn stripe_future_timestamp_is_invalid() {
    let verdict = run(
        &preset("stripe"),
        STRIPE_SECRET,
        &stripe_headers(STRIPE_TIMESTAMP, &format!("v1={STRIPE_SIGNATURE}")),
        STRIPE_BODY,
        STRIPE_TIMESTAMP - 301,
    );
    expect(
        verdict,
        Verification::Invalid,
        Reason::TimestampOutOfTolerance,
    );
}

#[test]
fn stripe_altered_timestamp_breaks_the_signature() {
    let verdict = run(
        &preset("stripe"),
        STRIPE_SECRET,
        &stripe_headers(STRIPE_TIMESTAMP + 10, &format!("v1={STRIPE_SIGNATURE}")),
        STRIPE_BODY,
        STRIPE_TIMESTAMP + 10,
    );
    expect(verdict, Verification::Invalid, Reason::Mismatch);
}

#[test]
fn stripe_duplicate_timestamp_is_invalid() {
    let value = format!("t={STRIPE_TIMESTAMP},t={STRIPE_TIMESTAMP},v1={STRIPE_SIGNATURE}");
    let verdict = run(
        &preset("stripe"),
        STRIPE_SECRET,
        &headers(&[("stripe-signature", &value)]),
        STRIPE_BODY,
        STRIPE_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::TimestampMalformed);
}

#[test]
fn stripe_without_a_timestamp_is_invalid() {
    let value = format!("v1={STRIPE_SIGNATURE}");
    let verdict = run(
        &preset("stripe"),
        STRIPE_SECRET,
        &headers(&[("stripe-signature", &value)]),
        STRIPE_BODY,
        STRIPE_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::TimestampAbsent);
}

#[test]
fn slack_published_vector_is_valid() {
    let verdict = run(
        &preset("slack"),
        SLACK_SECRET,
        &slack_headers(&SLACK_TIMESTAMP.to_string(), SLACK_SIGNATURE),
        SLACK_BODY,
        SLACK_TIMESTAMP + 60,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn slack_timestamp_that_is_not_a_number_is_invalid() {
    let verdict = run(
        &preset("slack"),
        SLACK_SECRET,
        &slack_headers("+1531420618", SLACK_SIGNATURE),
        SLACK_BODY,
        SLACK_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::TimestampMalformed);
}

#[test]
fn slack_signature_without_a_timestamp_header_is_invalid() {
    let verdict = run(
        &preset("slack"),
        SLACK_SECRET,
        &headers(&[("x-slack-signature", SLACK_SIGNATURE)]),
        SLACK_BODY,
        SLACK_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::TimestampAbsent);
}

#[test]
fn repeated_timestamp_header_is_invalid() {
    let timestamp = SLACK_TIMESTAMP.to_string();
    let repeated = headers(&[
        ("x-slack-signature", SLACK_SIGNATURE),
        ("x-slack-request-timestamp", &timestamp),
        ("x-slack-request-timestamp", &timestamp),
    ]);
    let verdict = run(
        &preset("slack"),
        SLACK_SECRET,
        &repeated,
        SLACK_BODY,
        SLACK_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::TimestampMalformed);
}

#[test]
fn standard_webhooks_published_vector_is_valid() {
    let verdict = run(
        &preset("standard_webhooks"),
        STANDARD_SECRET,
        &standard_headers(
            STANDARD_ID,
            &STANDARD_TIMESTAMP.to_string(),
            STANDARD_SIGNATURE,
        ),
        STANDARD_BODY,
        STANDARD_TIMESTAMP,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn standard_webhooks_list_skips_other_versions_and_finds_the_match() {
    let list = format!("v1a,AAAA v1,{} {STANDARD_SIGNATURE}", "AA==");
    let verdict = run(
        &preset("standard_webhooks"),
        STANDARD_SECRET,
        &standard_headers(STANDARD_ID, &STANDARD_TIMESTAMP.to_string(), &list),
        STANDARD_BODY,
        STANDARD_TIMESTAMP,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn standard_webhooks_message_id_is_part_of_the_signed_content() {
    let verdict = run(
        &preset("standard_webhooks"),
        STANDARD_SECRET,
        &standard_headers(
            "msg_other",
            &STANDARD_TIMESTAMP.to_string(),
            STANDARD_SIGNATURE,
        ),
        STANDARD_BODY,
        STANDARD_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::Mismatch);
}

#[test]
fn standard_webhooks_without_a_message_id_is_invalid() {
    let without_id = headers(&[
        ("webhook-timestamp", &STANDARD_TIMESTAMP.to_string()),
        ("webhook-signature", STANDARD_SIGNATURE),
    ]);
    let verdict = run(
        &preset("standard_webhooks"),
        STANDARD_SECRET,
        &without_id,
        STANDARD_BODY,
        STANDARD_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::MessageIdAbsent);
}

#[test]
fn standard_webhooks_secret_that_is_not_base64_cannot_verify() {
    let verdict = run(
        &preset("standard_webhooks"),
        "whsec_***",
        &standard_headers(
            STANDARD_ID,
            &STANDARD_TIMESTAMP.to_string(),
            STANDARD_SIGNATURE,
        ),
        STANDARD_BODY,
        STANDARD_TIMESTAMP,
    );
    expect(verdict, Verification::Invalid, Reason::SecretUnusable);
}

#[test]
fn generic_sha1_hex_over_the_body_is_valid() {
    let verifier = generic("x-sig", "sha1", "hex", None, None);
    let verdict = run(
        &verifier,
        "k1",
        &headers(&[("x-sig", SHA1_SIGNATURE)]),
        "payload",
        0,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn generic_sha512_base64_with_prefix_over_timestamp_and_body_is_valid() {
    let verifier = generic("x-sig", "sha512", "base64", Some("sig="), Some("x-ts"));
    let signature = format!("sig={SHA512_SIGNATURE}");
    let verdict = run(
        &verifier,
        "k2",
        &headers(&[("x-sig", &signature), ("x-ts", "1700000000")]),
        "payload",
        GENERIC_TIMESTAMP + 299,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn generic_timestamped_scheme_refuses_a_stale_request() {
    let verifier = generic("x-sig", "sha512", "base64", Some("sig="), Some("x-ts"));
    let signature = format!("sig={SHA512_SIGNATURE}");
    let verdict = run(
        &verifier,
        "k2",
        &headers(&[("x-sig", &signature), ("x-ts", "1700000000")]),
        "payload",
        GENERIC_TIMESTAMP + 3600,
    );
    expect(
        verdict,
        Verification::Invalid,
        Reason::TimestampOutOfTolerance,
    );
}

#[test]
fn generic_scheme_without_a_timestamp_header_ignores_time() {
    let verifier = generic("x-sig", "sha256", "hex", None, None);
    let verdict = run(
        &verifier,
        "k3",
        &headers(&[("x-sig", SHA256_SIGNATURE)]),
        "payload",
        i64::from(i32::MAX),
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn uppercase_hex_decodes_to_the_same_signature() {
    let verifier = generic("x-sig", "sha256", "hex", None, None);
    let verdict = run(
        &verifier,
        "k3",
        &headers(&[("x-sig", &SHA256_SIGNATURE.to_uppercase())]),
        "payload",
        0,
    );
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn oversized_signature_text_is_invalid_without_work() {
    let verifier = generic("x-sig", "sha256", "hex", None, None);
    let verdict = run(
        &verifier,
        "k3",
        &headers(&[("x-sig", &"a".repeat(4096))]),
        "payload",
        0,
    );
    expect(verdict, Verification::Invalid, Reason::SignatureMalformed);
}

#[test]
fn many_repeated_signature_headers_stay_bounded() {
    let verifier = generic("x-sig", "sha256", "hex", None, None);
    let mut map = HeaderMap::new();
    for _ in 0..100 {
        map.append("x-sig", HeaderValue::from_static("00"));
    }
    map.append("x-sig", HeaderValue::from_static(SHA256_SIGNATURE));
    let verdict = run(&verifier, "k3", &map, "payload", 0);
    expect(verdict, Verification::Invalid, Reason::SignatureMalformed);
}

fn violation(spec: &VerifierInput) -> Vec<(&'static str, ConfigProblem)> {
    Verifier::new(spec)
        .expect_err("must be refused")
        .into_iter()
        .map(|issue| (issue.field, issue.problem))
        .collect()
}

#[test]
fn generic_scheme_requires_header_algorithm_and_encoding() {
    assert_eq!(
        violation(&input("hmac")),
        vec![
            ("header", ConfigProblem::Required),
            ("algorithm", ConfigProblem::Required),
            ("encoding", ConfigProblem::Required),
        ]
    );
}

#[test]
fn preset_refuses_a_field_that_contradicts_it() {
    let mut spec = input("github");
    spec.algorithm = Some("sha1".to_owned());
    spec.header = Some("X-Hub-Signature-256".to_owned());
    assert_eq!(
        violation(&spec),
        vec![("algorithm", ConfigProblem::NotAllowed)]
    );
}

#[test]
fn unknown_scheme_algorithm_and_malformed_names_are_refused() {
    let mut spec = input("md5-magic");
    spec.secret_name = "lower".to_owned();
    assert_eq!(
        violation(&spec),
        vec![
            ("scheme", ConfigProblem::InvalidFormat),
            ("secret_name", ConfigProblem::InvalidFormat),
        ]
    );
    let mut generic_spec = input("hmac");
    generic_spec.header = Some("bad header".to_owned());
    generic_spec.algorithm = Some("md5".to_owned());
    generic_spec.encoding = Some("hex".to_owned());
    generic_spec.prefix = Some("has space".to_owned());
    assert_eq!(
        violation(&generic_spec),
        vec![
            ("header", ConfigProblem::InvalidFormat),
            ("algorithm", ConfigProblem::InvalidFormat),
            ("prefix", ConfigProblem::InvalidFormat),
        ]
    );
}

#[test]
fn every_preset_resolves_and_round_trips_through_its_document() {
    for scheme in ["github", "stripe", "slack", "shopify", "standard_webhooks"] {
        let verifier = preset(scheme);
        let document = verifier.document();
        let text = serde_json::to_string(&document).expect("serializes");
        let parsed: VerifierDocument = serde_json::from_str(&text).expect("parses");
        assert_eq!(Verifier::from_document(&parsed).expect("valid"), verifier);
        assert!(document.header.is_some() && document.algorithm.is_some());
    }
}

#[test]
fn document_with_an_unknown_field_is_not_read() {
    let text = r#"{"scheme":"github","secret_name":"S","header":null,"algorithm":null,"encoding":null,"prefix":null,"timestamp_header":null,"extra":1}"#;
    assert!(serde_json::from_str::<VerifierDocument>(text).is_err());
}

fn repeated_signatures(fillers: usize) -> HeaderMap {
    let mut map = HeaderMap::new();
    for _ in 0..fillers {
        map.append("x-sig", HeaderValue::from_static("00"));
    }
    map.append("x-sig", HeaderValue::from_static(SHA256_SIGNATURE));
    map
}

#[test]
fn signature_at_the_last_allowed_position_is_found() {
    let verifier = generic("x-sig", "sha256", "hex", None, None);
    let verdict = run(&verifier, "k3", &repeated_signatures(15), "payload", 0);
    expect(verdict, Verification::Valid, Reason::Matched);
}

#[test]
fn signature_past_the_item_limit_is_invalid_by_the_limit_rule() {
    let verifier = generic("x-sig", "sha256", "hex", None, None);
    let verdict = run(&verifier, "k3", &repeated_signatures(16), "payload", 0);
    expect(verdict, Verification::Invalid, Reason::SignatureMalformed);
}
