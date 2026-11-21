use time::UtcOffset;
use whl_template::{RenderErrorCode, RenderTarget};

use crate::common::{fixture, render, render_clean, render_in};

#[test]
fn upper_and_lower_use_full_unicode_case_mapping() {
    assert_eq!(render_clean("{{ request.query.name | upper }}"), "ZOË & CO");
    assert_eq!(render_clean("{{ request.query.name | lower }}"), "zoë & co");
    assert_eq!(render_clean("{{ \"straße\" | upper }}"), "STRASSE");
    assert_eq!(render_clean("{{ var.absent | upper }}"), "");
}

#[test]
fn trim_removes_unicode_whitespace_on_both_sides() {
    assert_eq!(render_clean("[{{ var.padded | trim }}]"), "[hi]");
    assert_eq!(render_clean("[{{ \"\u{a0}x\u{2003}\" | trim }}]"), "[x]");
    assert_eq!(render_clean("[{{ var.padded | upper | trim }}]"), "[HI]");
}

#[test]
fn truncate_counts_characters_not_bytes() {
    assert_eq!(
        render_clean("{{ request.query.name | truncate(3) }}"),
        "Zoë"
    );
    assert_eq!(render_clean("{{ request.query.name | truncate(0) }}"), "");
    assert_eq!(
        render_clean("{{ request.query.name | truncate(100) }}"),
        "Zoë & co"
    );
    assert_eq!(render_clean("{{ \"héllo\" | upper | truncate(2) }}"), "HÉ");
}

#[test]
fn truncate_with_a_negative_runtime_argument_is_an_error() {
    let mut context = fixture();
    context.vars.insert("neg".to_owned(), serde_json::json!(-1));
    let outcome = render_in(
        "[{{ \"abc\" | truncate(var.neg) }}]",
        RenderTarget::Body,
        &context,
    );
    assert_eq!(outcome.output(), "[]");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidArgument);
}

#[test]
fn replace_is_literal_and_replaces_every_occurrence() {
    assert_eq!(
        render_clean("{{ \"a.b.c\" | replace(\".\", \"-\") }}"),
        "a-b-c"
    );
    assert_eq!(render_clean("{{ \"a*b\" | replace(\"*\", \"\") }}"), "ab");
    assert_eq!(render_clean("{{ \"aaa\" | replace(\"aa\", \"b\") }}"), "ba");
    assert_eq!(render_clean("{{ \"x\" | replace(\"y\", \"z\") }}"), "x");
    assert_eq!(render_clean("{{ \"1+1\" | replace(\"1+1\", var.n) }}"), "5");
}

#[test]
fn replace_with_an_empty_runtime_search_is_an_error() {
    let outcome = render("{{ \"abc\" | replace(var.empty, \"x\") }}");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidArgument);
}

#[test]
fn length_counts_characters() {
    assert_eq!(render_clean("{{ request.query.name | length }}"), "8");
    assert_eq!(render_clean("{{ \"日本語\" | length }}"), "3");
    assert_eq!(render_clean("{{ var.absent | length }}"), "0");
    assert_eq!(render_clean("{{ 12345 | length }}"), "5");
}

#[test]
fn json_filter_writes_a_quoted_json_string() {
    assert_eq!(
        render_clean("{{ request.query.name | json }}"),
        "\"Zoë & co\""
    );
    assert_eq!(
        render_clean("{{ \"a\\\"b\\\\c\\nd\\te\" | json }}"),
        "\"a\\\"b\\\\c\\nd\\te\""
    );
    assert_eq!(render_clean("{{ \"\\r\" | json }}"), "\"\\r\"");
    assert_eq!(render_clean("{{ \"\u{1}\" | json }}"), "\"\\u0001\"");
    assert_eq!(render_clean("{{ \"\u{2028}\" | json }}"), "\"\\u2028\"");
    assert_eq!(render_clean("{{ var.absent | json }}"), "\"\"");
}

#[test]
fn url_filter_percent_encodes_everything_but_unreserved() {
    assert_eq!(
        render_clean("{{ request.query.name | url }}"),
        "Zo%C3%AB%20%26%20co"
    );
    assert_eq!(render_clean("{{ \"a-b_c.d~e\" | url }}"), "a-b_c.d~e");
    assert_eq!(render_clean("{{ \"/?#[]@\" | url }}"), "%2F%3F%23%5B%5D%40");
}

#[test]
fn html_filter_escapes_markup_characters() {
    assert_eq!(
        render_clean("{{ \"<a href=\\\"x\\\">'&</a>\" | html }}"),
        "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&lt;/a&gt;"
    );
}

#[test]
fn base64_round_trips_and_accepts_missing_padding() {
    assert_eq!(render_clean("{{ \"hello\" | base64 }}"), "aGVsbG8=");
    assert_eq!(render_clean("{{ \"\" | base64 }}"), "");
    assert_eq!(render_clean("{{ \"aGVsbG8=\" | base64_decode }}"), "hello");
    assert_eq!(render_clean("{{ \"aGVsbG8\" | base64_decode }}"), "hello");
    assert_eq!(
        render_clean("{{ \"héllo\" | base64 | base64_decode }}"),
        "héllo"
    );
}

#[test]
fn base64_decode_rejects_garbage_and_replaces_invalid_utf8() {
    let outcome = render("a{{ \"***\" | base64_decode }}b");
    assert_eq!(outcome.output(), "ab");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidBase64);
    assert_eq!(render_clean("{{ \"/w==\" | base64_decode }}"), "\u{fffd}");
}

#[test]
fn hex_filter_encodes_utf8_bytes() {
    assert_eq!(render_clean("{{ \"hi\" | hex }}"), "6869");
    assert_eq!(render_clean("{{ \"é\" | hex }}"), "c3a9");
}

#[test]
fn sha256_and_hmac_match_known_vectors() {
    assert_eq!(
        render_clean("{{ \"abc\" | sha256 }}"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        render_clean("{{ var.text | hmac_sha256(\"key\") }}"),
        "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
    );
    assert_eq!(
        render_clean("{{ \"\" | hmac_sha256(\"\") }}"),
        "b613679a0814d9ec772f95d778c35fc5ff1697c493715653c6c712144292c5ad"
    );
}

#[test]
fn hmac_key_may_come_from_a_secret() {
    let keyed = render_clean("{{ \"abc\" | hmac_sha256(secret.TOKEN) }}");
    let literal = render_clean("{{ \"abc\" | hmac_sha256(\"s3cret\") }}");
    assert_eq!(keyed, literal);
}

#[test]
fn default_replaces_missing_and_empty_values() {
    assert_eq!(render_clean("{{ var.absent | default(\"d\") }}"), "d");
    assert_eq!(render_clean("{{ var.empty | default(\"d\") }}"), "d");
    assert_eq!(render_clean("{{ var.nul | default(\"d\") }}"), "d");
    assert_eq!(render_clean("{{ var.n | default(\"d\") }}"), "5");
    assert_eq!(render_clean("{{ var.absent | default(var.n) }}"), "5");
    assert_eq!(render_clean("{{ var.absent | default(7) }}"), "7");
    assert_eq!(
        render_clean("{{ var.absent | default(var.other) | default(\"z\") }}"),
        "z"
    );
    let outcome = render("{{ var.absent | default(\"d\") }}");
    assert!(outcome.missing().is_empty());
}

#[test]
fn date_formats_in_utc() {
    let cases = [
        ("%Y-%m-%d %H:%M:%S", "2024-03-05 14:07:09"),
        ("%a %A %b %B", "Tue Tuesday Mar March"),
        ("%j %u %p %I %y", "065 2 PM 02 24"),
        ("[%e]", "[ 5]"),
        ("%s", "1709647629"),
        ("%F %T", "2024-03-05 14:07:09"),
        ("%L %f %z %%", "000 000000 +0000 %"),
        ("plain text", "plain text"),
    ];
    for (format, expected) in cases {
        let source = format!("{{{{ capture.received_at | date(\"{format}\") }}}}");
        assert_eq!(render_clean(&source), expected, "{format}");
    }
}

#[test]
fn date_converts_offsets_to_utc_and_accepts_unix_seconds() {
    let mut context = fixture();
    let offset = UtcOffset::from_hms(3, 0, 0).expect("offset");
    context.now = context.now.to_offset(offset);
    let outcome = render_in("{{ now | date(\"%H\") }}", RenderTarget::Body, &context);
    assert_eq!(outcome.output(), "14");
    assert_eq!(render_clean("{{ 0 | date(\"%Y\") }}"), "1970");
    assert_eq!(render_clean("{{ \"86400\" | date(\"%F\") }}"), "1970-01-02");
    assert_eq!(render_clean("{{ var.absent | date(\"%Y\") }}"), "");
}

#[test]
fn date_rejects_values_that_are_not_timestamps() {
    let outcome = render("{{ \"yesterday\" | date(\"%Y\") }}");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::TypeMismatch);
    let outcome = render("{{ 99999999999999999 | date(\"%Y\") }}");
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidArgument);
}

#[test]
fn date_runtime_format_errors_are_reported() {
    let mut context = fixture();
    context
        .vars
        .insert("fmt".to_owned(), serde_json::json!("%Q"));
    let outcome = render_in("{{ now | date(var.fmt) }}", RenderTarget::Body, &context);
    assert_eq!(outcome.errors()[0].code, RenderErrorCode::InvalidArgument);
}

#[test]
fn filters_chain_left_to_right() {
    assert_eq!(
        render_clean("{{ \" Zoë \" | trim | upper | truncate(2) | url }}"),
        "ZO"
    );
    assert_eq!(
        render_clean("{{ request.query.name | url | upper }}"),
        "ZO%C3%AB%20%26%20CO"
    );
}
