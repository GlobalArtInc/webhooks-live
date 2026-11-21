use whl_core::endpoint_id::EndpointId;
use whl_secrets::{
    KeyId, Keyring, KeyringError, KeyringProblem, MAC_BYTES, MacError, PLACEHOLDER, Redactor,
    Secret, SecretName, constant_time_eq,
};

const KEY_ONE: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const KEY_TWO: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";
const SEQUENTIAL_KEY: &str = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA=";
const VALUE: &str = "sk_live_correct-horse-battery-staple";

fn problem(spec: &str) -> KeyringProblem {
    match Keyring::parse(spec).expect_err("the key set is invalid") {
        KeyringError::Entry { problem, .. } => problem,
        other => panic!("expected an entry problem, got {other:?}"),
    }
}

fn endpoint(text: &str) -> EndpointId {
    EndpointId::parse(text).expect("a valid endpoint id")
}

#[test]
fn the_key_set_is_parsed_in_order_and_the_first_entry_is_active() {
    let ring = Keyring::parse(&format!(" 7:{KEY_ONE} , 3:{KEY_TWO} ")).expect("valid");

    assert_eq!(ring.active_id().unpack(), 7);
    assert_eq!(
        ring.key_ids()
            .iter()
            .map(|id| id.unpack())
            .collect::<Vec<_>>(),
        [3, 7]
    );
    assert!(ring.contains(KeyId::new(3).expect("id")));
    assert!(!ring.contains(KeyId::new(4).expect("id")));
}

#[test]
fn a_malformed_key_set_is_refused_with_the_entry_and_the_reason() {
    assert_eq!(Keyring::parse("").expect_err("empty"), KeyringError::Empty);
    assert_eq!(
        Keyring::parse(" , ").expect_err("blank"),
        KeyringError::Empty
    );
    assert_eq!(problem("nocolon"), KeyringProblem::MissingSeparator);
    for id in ["0", "32768", "x", "+1", "-1", ""] {
        assert_eq!(
            problem(&format!("{id}:{KEY_ONE}")),
            KeyringProblem::InvalidId,
            "{id}"
        );
    }
    assert_eq!(problem("1:not base64!"), KeyringProblem::InvalidBase64);
    assert_eq!(problem("1:AAAA"), KeyringProblem::WrongLength);
    assert_eq!(
        problem(&format!("1:{}", "A".repeat(88))),
        KeyringProblem::WrongLength
    );
    assert_eq!(
        problem(&format!("1:{KEY_ONE},1:{KEY_TWO}")),
        KeyringProblem::DuplicateId(KeyId::new(1).expect("id"))
    );
    assert_eq!(
        Keyring::parse(&format!("1:{KEY_ONE},2:AAAA")).expect_err("second entry"),
        KeyringError::Entry {
            position: 2,
            problem: KeyringProblem::WrongLength
        }
    );
}

#[test]
fn key_material_never_appears_in_errors_or_debug_output() {
    let ring = Keyring::parse(&format!("1:{KEY_ONE}")).expect("valid");
    let errors = [
        Keyring::parse(&format!("1:{KEY_ONE}x"))
            .expect_err("bad length")
            .to_string(),
        Keyring::parse(&format!("x:{KEY_ONE}"))
            .expect_err("bad id")
            .to_string(),
        format!(
            "{:?}",
            Keyring::parse(&format!("1:{KEY_ONE}x")).expect_err("bad length")
        ),
    ];

    let shown = format!("{ring:?}");

    assert!(shown.contains("active"));
    for text in errors.iter().chain([&shown]) {
        assert!(!text.contains(KEY_ONE), "{text}");
        assert!(!text.contains("AQEB"), "{text}");
    }
}

#[test]
fn debug_and_display_of_a_secret_print_a_fixed_placeholder() {
    let secret = Secret::new(VALUE.to_owned()).expect("valid");
    let name = SecretName::parse("API_KEY").expect("valid");
    let redactor = Redactor::new([(&name, &secret)]).expect("builds");

    for text in [
        format!("{secret}"),
        format!("{secret:?}"),
        format!("{secret:#?}"),
        format!("{:?}", Some(&secret)),
        format!("{redactor:?}"),
    ] {
        assert!(!text.contains(VALUE), "{text}");
        assert!(!text.contains("sk_live"), "{text}");
    }
    assert_eq!(format!("{secret}"), PLACEHOLDER);
    assert_eq!(format!("{secret:?}"), PLACEHOLDER);
}

#[test]
fn the_constant_time_helper_returns_the_right_result_for_equal_and_unequal_inputs() {
    assert!(constant_time_eq(b"abcd", b"abcd"));
    assert!(constant_time_eq(b"", b""));
    assert!(!constant_time_eq(b"abcd", b"abce"));
    assert!(!constant_time_eq(b"abcd", b"abc"));
    assert!(!constant_time_eq(b"abc", b"abcd"));
    assert!(!constant_time_eq(&[0, 0, 0, 1], &[0, 0, 0, 0]));
    let one = Secret::new(VALUE.to_owned()).expect("valid");
    let same = Secret::new(VALUE.to_owned()).expect("valid");
    let other = Secret::new(format!("{VALUE}!")).expect("valid");
    assert!(one == same);
    assert!(one != other);
}

#[test]
fn the_capture_mac_key_follows_the_documented_derivation() {
    let ring = Keyring::parse(&format!("1:{SEQUENTIAL_KEY}")).expect("valid");
    let endpoint = endpoint("0123456789abcdefghjkmnpq");
    let (id, key) = ring.active_capture_mac_key(&endpoint).expect("derives");

    let tag = key.tag(b"correct horse").expect("tags");

    assert_eq!(id.unpack(), 1);
    assert_eq!(tag.unpack().len(), MAC_BYTES);
    let hex: String = tag
        .unpack()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        hex,
        "c960a7acb98fa5e2b25863142ccd94e754b658649798e911a008a1b3a188fea5"
    );
    assert!(key.verify(b"correct horse", tag.unpack()));
    assert!(!key.verify(b"correct horsf", tag.unpack()));
    assert!(!key.verify(b"correct horse", &tag.unpack()[..MAC_BYTES - 1]));
    assert!(!key.verify(b"correct horse", &[]));
    assert!(!format!("{key:?}{tag:?}").contains("c960"));
}

#[test]
fn the_capture_mac_key_differs_per_endpoint_and_per_key() {
    let ring = Keyring::parse(&format!("1:{KEY_ONE},2:{KEY_TWO}")).expect("valid");
    let first = endpoint("0123456789abcdefghjkmnpq");
    let second = endpoint("qpnmkjhgfedcba9876543210");
    let id = |value| KeyId::new(value).expect("id");
    let tag = |key: &str, endpoint: &EndpointId, key_id| {
        ring.capture_mac_key(key_id, endpoint)
            .expect("derives")
            .tag(key.as_bytes())
            .expect("tags")
    };

    assert_eq!(tag("v", &first, id(1)), tag("v", &first, id(1)));
    assert_ne!(tag("v", &first, id(1)), tag("v", &second, id(1)));
    assert_ne!(tag("v", &first, id(1)), tag("v", &first, id(2)));
    assert_eq!(
        ring.capture_mac_key(id(9), &first)
            .expect_err("no such key"),
        MacError::UnknownKey(id(9))
    );
}
