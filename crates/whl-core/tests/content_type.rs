use whl_core::content_type::ContentType;
use whl_core::validation::ViolationCode;

fn codes(text: &str) -> Vec<ViolationCode> {
    ContentType::parse(text)
        .expect_err("content type is invalid")
        .codes()
        .to_vec()
}

#[test]
fn accepts_media_types_with_and_without_parameters() {
    for text in [
        "text/plain",
        "application/json",
        "text/html; charset=utf-8",
        "text/plain;charset=utf-8",
        "multipart/form-data; boundary=\"a;b c\"",
        "application/vnd.api+json",
        "text/plain; a=\"q\\\"uote\"; b=c",
    ] {
        let parsed = ContentType::parse(text).expect(text);

        assert_eq!(parsed.unpack(), text);
    }
}

#[test]
fn default_is_text_plain() {
    assert_eq!(ContentType::default().unpack(), "text/plain");
}

#[test]
fn rejects_empty() {
    assert_eq!(codes(""), [ViolationCode::Empty]);
}

#[test]
fn rejects_missing_or_extra_slash_and_empty_parts() {
    for text in ["text", "/plain", "text/", "text/plain/extra", "/"] {
        assert_eq!(codes(text), [ViolationCode::InvalidMediaType], "{text:?}");
    }
}

#[test]
fn rejects_non_token_characters_and_wildcards_in_type_and_subtype() {
    for text in [
        "te xt/plain",
        "text/pl ain",
        "text/pl(ain)",
        "*/*",
        "text/*",
        "t\u{e9}xt/plain",
    ] {
        assert_eq!(codes(text), [ViolationCode::InvalidMediaType], "{text:?}");
    }
}

#[test]
fn rejects_surrounding_whitespace_without_trimming() {
    for text in [
        " text/plain",
        "text/plain ",
        "text/plain ; a=b",
        "text /plain",
    ] {
        assert_eq!(codes(text), [ViolationCode::InvalidMediaType], "{text:?}");
    }
}

#[test]
fn rejects_malformed_parameters() {
    for text in [
        "text/plain;",
        "text/plain; ",
        "text/plain; charset",
        "text/plain; =utf-8",
        "text/plain; charset=",
        "text/plain; a=\"unterminated",
        "text/plain; a=\"x\"y",
        "text/plain; a=b c",
        "text/plain; a b=c",
    ] {
        assert_eq!(codes(text), [ViolationCode::InvalidMediaType], "{text:?}");
    }
}

#[test]
fn rejects_cr_lf_nul_tab_and_other_control_characters() {
    for text in [
        "text/plain\r\nX-Injected: 1",
        "text/plain\n",
        "text/plain\0",
        "text/plain;\tcharset=utf-8",
        "text/plain\x7f",
        "text/plain\u{85}",
    ] {
        assert_eq!(codes(text), [ViolationCode::ControlCharacter], "{text:?}");
    }
}

#[test]
fn rejects_over_255_bytes_and_accepts_exactly_255() {
    let at_limit = format!("text/{}", "a".repeat(250));
    let over_limit = format!("text/{}", "a".repeat(251));

    assert!(ContentType::parse(&at_limit).is_ok());
    assert_eq!(codes(&over_limit), [ViolationCode::TooLong { max: 255 }]);
}

#[test]
fn allows_active_types_because_the_capture_response_is_sandboxed() {
    for text in [
        "text/html",
        "image/svg+xml",
        "application/xhtml+xml",
        "text/javascript",
    ] {
        assert!(ContentType::parse(text).is_ok(), "{text}");
    }
}
