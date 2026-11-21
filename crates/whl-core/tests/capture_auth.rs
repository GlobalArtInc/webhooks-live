use whl_core::capture_auth::{
    AuthHeaderName, AuthHeaderNameError, AuthUsername, CredentialError, MAX_CREDENTIAL_BYTES,
    MIN_SECRET_BYTES, check_basic_password, check_header_value,
};

#[test]
fn a_basic_password_is_eight_to_256_visible_ascii_bytes() {
    assert_eq!(check_basic_password(""), Err(CredentialError::Empty));
    assert_eq!(
        check_basic_password(&"a".repeat(MIN_SECRET_BYTES - 1)),
        Err(CredentialError::TooShort)
    );
    assert!(check_basic_password(&"a".repeat(MIN_SECRET_BYTES)).is_ok());
    assert!(check_basic_password(&"a".repeat(MAX_CREDENTIAL_BYTES)).is_ok());
    assert_eq!(
        check_basic_password(&"a".repeat(MAX_CREDENTIAL_BYTES + 1)),
        Err(CredentialError::TooLong)
    );
    for text in [
        "has a space",
        "tab\there-x",
        "line\nbreak-x",
        "caf\u{e9}-caf\u{e9}",
        "\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}\u{7f}",
    ] {
        assert_eq!(
            check_basic_password(text),
            Err(CredentialError::NotVisibleAscii),
            "{text:?}"
        );
    }
}

#[test]
fn a_header_value_may_hold_inner_spaces_but_not_edge_spaces_or_control_bytes() {
    assert!(check_header_value("Bearer test-token-one").is_ok());
    assert_eq!(check_header_value(""), Err(CredentialError::Empty));
    assert_eq!(
        check_header_value(&"a".repeat(MIN_SECRET_BYTES - 1)),
        Err(CredentialError::TooShort)
    );
    assert_eq!(
        check_header_value(&"a".repeat(MAX_CREDENTIAL_BYTES + 1)),
        Err(CredentialError::TooLong)
    );
    for text in [" leading-space", "trailing-space "] {
        assert_eq!(
            check_header_value(text),
            Err(CredentialError::EdgeSpace),
            "{text:?}"
        );
    }
    for text in ["tab\tinside-x", "line\nbreak-x", "caf\u{e9}-caf\u{e9}"] {
        assert_eq!(
            check_header_value(text),
            Err(CredentialError::NotPrintableAscii),
            "{text:?}"
        );
    }
}

#[test]
fn a_basic_username_cannot_hold_a_colon() {
    assert_eq!(
        AuthUsername::parse("alice").expect("valid").unpack(),
        "alice"
    );
    assert_eq!(AuthUsername::parse("al:ice"), Err(CredentialError::Colon));
    assert_eq!(AuthUsername::parse(""), Err(CredentialError::Empty));
}

#[test]
fn a_header_name_is_a_lowercased_token_of_at_most_64_bytes() {
    assert_eq!(
        AuthHeaderName::parse("X-Hook-Token")
            .expect("valid")
            .unpack(),
        "x-hook-token"
    );
    assert_eq!(AuthHeaderName::parse(""), Err(AuthHeaderNameError::Empty));
    assert_eq!(
        AuthHeaderName::parse("bad name"),
        Err(AuthHeaderNameError::InvalidToken)
    );
    assert!(AuthHeaderName::parse(&"a".repeat(64)).is_ok());
    assert_eq!(
        AuthHeaderName::parse(&"a".repeat(65)),
        Err(AuthHeaderNameError::TooLong)
    );
}

#[test]
fn a_header_that_proxies_rewrite_or_that_frames_the_message_is_refused() {
    for name in [
        "Host",
        "connection",
        "Content-Length",
        "Transfer-Encoding",
        "Proxy-Authorization",
    ] {
        assert_eq!(
            AuthHeaderName::parse(name),
            Err(AuthHeaderNameError::Denied),
            "{name}"
        );
    }
    assert!(AuthHeaderName::parse("Authorization").is_ok());
}
