use whl_core::status::{
    HttpStatus, first_segment_after_id, parse_status_override, status_override_from_path,
};

fn parsed(text: &str) -> Option<u16> {
    parse_status_override(text).map(|status| status.unpack())
}

#[test]
fn accepts_the_range_bounds_and_a_middle_value() {
    assert_eq!(parsed("100"), Some(100));
    assert_eq!(parsed("404"), Some(404));
    assert_eq!(parsed("599"), Some(599));
}

#[test]
fn rejects_values_outside_the_range() {
    assert_eq!(parsed("099"), None);
    assert_eq!(parsed("600"), None);
    assert_eq!(parsed("000"), None);
    assert_eq!(parsed("999"), None);
}

#[test]
fn rejects_four_digits() {
    assert_eq!(parsed("4040"), None);
}

#[test]
fn rejects_sign_whitespace_and_letters() {
    assert_eq!(parsed("+404"), None);
    assert_eq!(parsed("-404"), None);
    assert_eq!(parsed(" 404"), None);
    assert_eq!(parsed("404 "), None);
    assert_eq!(parsed("40a"), None);
    assert_eq!(parsed("0x1"), None);
}

#[test]
fn rejects_unicode_digits_and_empty_input() {
    assert_eq!(parsed("\u{664}\u{660}\u{664}"), None);
    assert_eq!(parsed("\u{ff14}\u{ff10}\u{ff14}"), None);
    assert_eq!(parsed(""), None);
}

#[test]
fn rejects_percent_encoded_digits() {
    assert_eq!(parsed("%34%30%34"), None);
}

#[test]
fn first_segment_is_taken_from_the_path_after_the_id() {
    assert_eq!(first_segment_after_id("/404/orders/1"), Some("404"));
    assert_eq!(first_segment_after_id("/404"), Some("404"));
    assert_eq!(first_segment_after_id("/404/"), Some("404"));
    assert_eq!(first_segment_after_id("/orders/404"), Some("orders"));
    assert_eq!(first_segment_after_id("//404"), Some(""));
    assert_eq!(first_segment_after_id("/"), Some(""));
}

#[test]
fn first_segment_requires_the_leading_slash() {
    assert_eq!(first_segment_after_id(""), None);
    assert_eq!(first_segment_after_id("404/orders"), None);
}

#[test]
fn override_from_path_applies_only_to_the_first_segment() {
    assert_eq!(
        status_override_from_path("/404/orders"),
        HttpStatus::new(404)
    );
    assert_eq!(status_override_from_path("/orders/404"), None);
    assert_eq!(status_override_from_path("//404"), None);
    assert_eq!(status_override_from_path(""), None);
}
