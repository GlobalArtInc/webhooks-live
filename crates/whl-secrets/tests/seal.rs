use std::collections::HashSet;

use whl_core::endpoint_id::EndpointId;
use whl_secrets::{
    KeyId, Keyring, MAX_CIPHERTEXT_BYTES, MAX_VALUE_BYTES, NONCE_BYTES, OpenError, SealedSecret,
    Secret, SecretName, TAG_BYTES, ValueError,
};

const KEY_ONE: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";
const KEY_TWO: &str = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI=";
const VALUE: &str = "sk_live_correct-horse-battery-staple";

fn ring(spec: &str) -> Keyring {
    Keyring::parse(spec).expect("the key set is valid")
}

fn endpoint(text: &str) -> EndpointId {
    EndpointId::parse(text).expect("a valid endpoint id")
}

fn first_endpoint() -> EndpointId {
    endpoint("0123456789abcdefghjkmnpq")
}

fn second_endpoint() -> EndpointId {
    endpoint("qpnmkjhgfedcba9876543210")
}

fn name(text: &str) -> SecretName {
    SecretName::parse(text).expect("a valid secret name")
}

fn secret(text: &str) -> Secret {
    Secret::new(text.to_owned()).expect("a valid secret value")
}

fn key(id: u16) -> KeyId {
    KeyId::new(id).expect("a valid key id")
}

fn tampered(
    sealed: &SealedSecret,
    mutate: impl FnOnce(&mut Vec<u8>, &mut Vec<u8>),
) -> SealedSecret {
    let mut nonce = sealed.nonce().to_vec();
    let mut ciphertext = sealed.ciphertext().to_vec();
    mutate(&mut nonce, &mut ciphertext);
    SealedSecret::from_parts(sealed.key_id(), &nonce, ciphertext)
        .expect("the parts are well formed")
}

#[test]
fn a_sealed_value_opens_back_to_the_same_value() {
    let ring = ring(&format!("1:{KEY_ONE}"));
    let sealed = ring
        .seal(&first_endpoint(), &name("API_KEY"), &secret(VALUE))
        .expect("seals");

    let opened = ring
        .open(&first_endpoint(), &name("API_KEY"), &sealed)
        .expect("opens");

    assert_eq!(opened.unpack(), VALUE);
    assert_eq!(sealed.key_id(), key(1));
    assert_eq!(sealed.nonce().len(), NONCE_BYTES);
    assert_eq!(sealed.ciphertext().len(), VALUE.len() + TAG_BYTES);
    assert!(
        !sealed
            .ciphertext()
            .windows(VALUE.len())
            .any(|window| window == VALUE.as_bytes())
    );
}

#[test]
fn the_largest_value_seals_within_the_documented_ciphertext_bound() {
    let ring = ring(&format!("1:{KEY_ONE}"));
    let value = secret(&"x".repeat(MAX_VALUE_BYTES));

    let sealed = ring
        .seal(&first_endpoint(), &name("BIG"), &value)
        .expect("seals");

    assert_eq!(sealed.ciphertext().len(), MAX_CIPHERTEXT_BYTES);
    assert_eq!(
        Secret::new("x".repeat(MAX_VALUE_BYTES + 1)).expect_err("too long"),
        ValueError::TooLong
    );
    assert_eq!(
        Secret::new(String::new()).expect_err("empty"),
        ValueError::Empty
    );
}

#[test]
fn a_flipped_bit_in_the_ciphertext_or_the_nonce_fails_to_open() {
    let ring = ring(&format!("1:{KEY_ONE}"));
    let sealed = ring
        .seal(&first_endpoint(), &name("API_KEY"), &secret(VALUE))
        .expect("seals");

    let bad_ciphertext = tampered(&sealed, |_, ciphertext| ciphertext[0] ^= 1);
    let bad_tag = tampered(&sealed, |_, ciphertext| {
        let last = ciphertext.len() - 1;
        ciphertext[last] ^= 0x80;
    });
    let bad_nonce = tampered(&sealed, |nonce, _| nonce[NONCE_BYTES - 1] ^= 1);

    for forged in [bad_ciphertext, bad_tag, bad_nonce] {
        assert_eq!(
            ring.open(&first_endpoint(), &name("API_KEY"), &forged)
                .expect_err("a forged value must not open"),
            OpenError::Authentication
        );
    }
}

#[test]
fn a_ciphertext_moved_to_another_endpoint_or_name_fails_to_open() {
    let ring = ring(&format!("1:{KEY_ONE}"));
    let sealed = ring
        .seal(&first_endpoint(), &name("API_KEY"), &secret(VALUE))
        .expect("seals");

    assert_eq!(
        ring.open(&second_endpoint(), &name("API_KEY"), &sealed)
            .expect_err("another endpoint"),
        OpenError::Authentication
    );
    assert_eq!(
        ring.open(&first_endpoint(), &name("OTHER_KEY"), &sealed)
            .expect_err("another name"),
        OpenError::Authentication
    );
}

#[test]
fn the_key_id_is_bound_into_the_associated_data() {
    let ring = ring(&format!("1:{KEY_ONE},2:{KEY_ONE}"));
    let sealed = ring
        .seal(&first_endpoint(), &name("API_KEY"), &secret(VALUE))
        .expect("seals");
    let relabelled = SealedSecret::from_parts(key(2), sealed.nonce(), sealed.ciphertext().to_vec())
        .expect("well formed");

    assert_eq!(
        ring.open(&first_endpoint(), &name("API_KEY"), &relabelled)
            .expect_err("same key bytes, other id"),
        OpenError::Authentication
    );
}

#[test]
fn a_key_id_missing_from_the_key_set_is_reported_by_id() {
    let writer = ring(&format!("1:{KEY_ONE}"));
    let reader = ring(&format!("2:{KEY_TWO}"));
    let sealed = writer
        .seal(&first_endpoint(), &name("API_KEY"), &secret(VALUE))
        .expect("seals");

    assert_eq!(
        reader
            .open(&first_endpoint(), &name("API_KEY"), &sealed)
            .expect_err("key 1 is gone"),
        OpenError::UnknownKey(key(1))
    );
}

#[test]
fn a_wrong_key_with_the_right_id_fails_to_open() {
    let writer = ring(&format!("1:{KEY_ONE}"));
    let reader = ring(&format!("1:{KEY_TWO}"));
    let sealed = writer
        .seal(&first_endpoint(), &name("API_KEY"), &secret(VALUE))
        .expect("seals");

    assert_eq!(
        reader
            .open(&first_endpoint(), &name("API_KEY"), &sealed)
            .expect_err("another key"),
        OpenError::Authentication
    );
}

#[test]
fn ten_thousand_seals_of_one_value_never_repeat_a_nonce_or_a_ciphertext() {
    let ring = ring(&format!("1:{KEY_ONE}"));
    let value = secret(VALUE);
    let mut nonces = HashSet::new();
    let mut ciphertexts = HashSet::new();

    for _ in 0..10_000 {
        let sealed = ring
            .seal(&first_endpoint(), &name("API_KEY"), &value)
            .expect("seals");
        assert!(nonces.insert(*sealed.nonce()));
        assert!(ciphertexts.insert(sealed.ciphertext().to_vec()));
    }
}

#[test]
fn the_first_key_writes_and_every_key_reads_and_reseal_moves_a_row_to_the_active_key() {
    let old = ring(&format!("1:{KEY_ONE}"));
    let rotated = ring(&format!("2:{KEY_TWO},1:{KEY_ONE}"));
    let before = old
        .seal(&first_endpoint(), &name("API_KEY"), &secret(VALUE))
        .expect("seals under the old key");

    assert_eq!(rotated.active_id(), key(2));
    assert_eq!(rotated.key_ids(), vec![key(1), key(2)]);
    assert_eq!(
        rotated
            .open(&first_endpoint(), &name("API_KEY"), &before)
            .expect("the retained key still reads")
            .unpack(),
        VALUE
    );
    let fresh = rotated
        .seal(&first_endpoint(), &name("NEW_KEY"), &secret(VALUE))
        .expect("seals");
    assert_eq!(fresh.key_id(), key(2));

    let after = rotated
        .reseal(&first_endpoint(), &name("API_KEY"), &before)
        .expect("reseals");

    assert_eq!(after.key_id(), key(2));
    assert_ne!(after.nonce(), before.nonce());
    assert_eq!(
        rotated
            .open(&first_endpoint(), &name("API_KEY"), &after)
            .expect("opens under the new key")
            .unpack(),
        VALUE
    );
    let new_only = ring(&format!("2:{KEY_TWO}"));
    assert_eq!(
        new_only
            .open(&first_endpoint(), &name("API_KEY"), &after)
            .expect("the old key is no longer needed")
            .unpack(),
        VALUE
    );
}

#[test]
fn sealed_parts_with_a_bad_nonce_or_ciphertext_length_are_refused() {
    assert!(SealedSecret::from_parts(key(1), &[0; 23], vec![0; 32]).is_err());
    assert!(SealedSecret::from_parts(key(1), &[0; 25], vec![0; 32]).is_err());
    assert!(SealedSecret::from_parts(key(1), &[0; 24], vec![0; TAG_BYTES]).is_err());
    assert!(SealedSecret::from_parts(key(1), &[0; 24], vec![0; MAX_CIPHERTEXT_BYTES + 1]).is_err());
    assert!(SealedSecret::from_parts(key(1), &[0; 24], vec![0; MAX_CIPHERTEXT_BYTES]).is_ok());
}

#[test]
fn a_secret_name_follows_the_documented_grammar() {
    for valid in ["A", "API_KEY", "K9", "A_", &format!("A{}", "B".repeat(63))] {
        assert!(SecretName::parse(valid).is_ok(), "{valid}");
    }
    for invalid in [
        "",
        "a",
        "_A",
        "9A",
        "API-KEY",
        "Api",
        "A B",
        "A\n",
        "ÄB",
        &format!("A{}", "B".repeat(64)),
    ] {
        assert!(SecretName::parse(invalid).is_err(), "{invalid:?}");
    }
}
