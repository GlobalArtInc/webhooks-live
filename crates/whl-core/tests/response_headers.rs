use std::time::{Duration, Instant};

use whl_core::header::HeaderInput;
use whl_core::response::{ResponseConfig, ResponseConfigInput};
use whl_core::validation::{FieldPath, MAX_VIOLATIONS, ValidationError, Violation, ViolationCode};

fn header(name: &str, value: &str) -> HeaderInput {
    HeaderInput {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

fn validate(headers: Vec<HeaderInput>) -> Result<ResponseConfig, ValidationError> {
    ResponseConfig::validate(ResponseConfigInput {
        status: 200,
        content_type: "text/plain".to_owned(),
        headers,
        body: Vec::new(),
        delay_ms: 0,
    })
}

fn violations(headers: Vec<HeaderInput>) -> Vec<Violation> {
    validate(headers)
        .expect_err("headers are invalid")
        .violations()
        .to_vec()
}

fn one(field: FieldPath, code: ViolationCode) -> Vec<Violation> {
    vec![Violation { field, code }]
}

#[test]
fn accepts_valid_headers_and_keeps_their_order_and_case() {
    let config = validate(vec![
        header("X-First", "a b\tc"),
        header("x-second", ""),
        header("X-Token!#$%&'*+-.^_`|~", "v"),
    ])
    .expect("valid headers");

    let names: Vec<_> = config
        .headers()
        .iter()
        .map(|h| h.name.unpack().to_owned())
        .collect();
    assert_eq!(names, ["X-First", "x-second", "X-Token!#$%&'*+-.^_`|~"]);
    assert_eq!(config.headers()[0].value.unpack(), "a b\tc");
}

#[test]
fn rejects_cr_in_value() {
    assert_eq!(
        violations(vec![header("X-A", "ok\rX-Injected: 1")]),
        one(FieldPath::HeaderValue(0), ViolationCode::ControlCharacter)
    );
}

#[test]
fn rejects_lf_in_value() {
    assert_eq!(
        violations(vec![header("X-A", "ok\nX-Injected: 1")]),
        one(FieldPath::HeaderValue(0), ViolationCode::ControlCharacter)
    );
}

#[test]
fn rejects_nul_and_del_in_value() {
    assert_eq!(
        violations(vec![header("X-A", "a\0b")]),
        one(FieldPath::HeaderValue(0), ViolationCode::ControlCharacter)
    );
    assert_eq!(
        violations(vec![header("X-A", "a\x7fb")]),
        one(FieldPath::HeaderValue(0), ViolationCode::ControlCharacter)
    );
}

#[test]
fn rejects_non_ascii_in_value() {
    assert_eq!(
        violations(vec![header("X-A", "caf\u{e9}")]),
        one(FieldPath::HeaderValue(0), ViolationCode::NonAscii)
    );
}

#[test]
fn rejects_value_over_1024_bytes_and_accepts_exactly_1024() {
    assert!(validate(vec![header("X-A", &"a".repeat(1024))]).is_ok());
    assert_eq!(
        violations(vec![header("X-A", &"a".repeat(1025))]),
        one(
            FieldPath::HeaderValue(0),
            ViolationCode::TooLong { max: 1024 }
        )
    );
}

#[test]
fn rejects_bad_name_tokens() {
    for name in ["X A", "X:A", "X-\u{e9}", "X\nA", "X/A", "X(A)", "X\"A\""] {
        assert_eq!(
            violations(vec![header(name, "v")]),
            one(FieldPath::HeaderName(0), ViolationCode::InvalidToken),
            "name {name:?}"
        );
    }
}

#[test]
fn rejects_empty_name_and_name_over_64_bytes() {
    assert_eq!(
        violations(vec![header("", "v")]),
        one(FieldPath::HeaderName(0), ViolationCode::Empty)
    );
    assert!(validate(vec![header(&"a".repeat(64), "v")]).is_ok());
    assert_eq!(
        violations(vec![header(&"a".repeat(65), "v")]),
        one(FieldPath::HeaderName(0), ViolationCode::TooLong { max: 64 })
    );
}

#[test]
fn rejects_set_cookie_in_any_case() {
    for name in ["Set-Cookie", "set-cookie", "SET-COOKIE", "sEt-CoOkIe"] {
        assert_eq!(
            violations(vec![header(name, "a=b")]),
            one(FieldPath::HeaderName(0), ViolationCode::Denied),
            "name {name}"
        );
    }
}

#[test]
fn rejects_every_denylisted_name_and_prefix_in_any_case() {
    let denied = [
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Keep-Alive",
        "Upgrade",
        "Trailer",
        "TE",
        "Host",
        "Proxy-Authorization",
        "Proxy-Connection",
        "Content-Security-Policy",
        "Content-Security-Policy-Report-Only",
        "X-Content-Type-Options",
        "Access-Control-Allow-Origin",
        "Access-Control-Max-Age",
        "X-Request-Id",
        "X-Endpoint-Id",
        "Content-Type",
    ];
    for name in denied {
        for variant in [name.to_owned(), name.to_uppercase(), name.to_lowercase()] {
            assert_eq!(
                violations(vec![header(&variant, "v")]),
                one(FieldPath::HeaderName(0), ViolationCode::Denied),
                "name {variant}"
            );
        }
    }
}

#[test]
fn rejects_session_and_origin_wide_policy_headers_in_mixed_case() {
    let denied = [
        "Set-Cookie2",
        "Clear-Site-Data",
        "Strict-Transport-Security",
        "Cache-Control",
        "Alt-Svc",
        "Refresh",
        "Content-Encoding",
        "X-Frame-Options",
        "Referrer-Policy",
        "Permissions-Policy",
        "Service-Worker-Allowed",
        "Public-Key-Pins",
        "Public-Key-Pins-Report-Only",
        "Report-To",
        "NEL",
        "Origin-Trial",
        "Document-Policy",
        "WWW-Authenticate",
        "Proxy-Authenticate",
        "Accept-CH",
        "Critical-CH",
        "Set-Login",
        "Cross-Origin-Opener-Policy",
        "Cross-Origin-Embedder-Policy",
        "Cross-Origin-Resource-Policy",
    ];
    for name in denied {
        let mixed: String = name
            .chars()
            .enumerate()
            .map(|(index, c)| {
                if index % 2 == 0 {
                    c.to_ascii_lowercase()
                } else {
                    c.to_ascii_uppercase()
                }
            })
            .collect();
        for variant in [name.to_owned(), name.to_uppercase(), mixed] {
            assert_eq!(
                violations(vec![header(&variant, "v")]),
                one(FieldPath::HeaderName(0), ViolationCode::Denied),
                "name {variant}"
            );
        }
    }
}

#[test]
fn allows_location_link_and_content_disposition() {
    assert!(
        validate(vec![
            header("Location", "https://example.test/next"),
            header("Link", "<https://example.test>; rel=next"),
            header("Content-Disposition", "attachment; filename=a.txt"),
        ])
        .is_ok()
    );
}

#[test]
fn does_not_deny_names_that_only_resemble_the_denylist() {
    assert!(
        validate(vec![
            header("X-Proxy-Id", "1"),
            header("Content-Language", "en"),
            header("Te-Extra", "1"),
            header("Hosted-By", "1"),
            header("Cache-Control-Extra", "1"),
            header("X-Cross-Origin-Id", "1"),
        ])
        .is_ok()
    );
}

#[test]
fn rejects_duplicate_names_compared_case_insensitively() {
    assert_eq!(
        violations(vec![
            header("X-A", "1"),
            header("x-b", "2"),
            header("x-a", "3")
        ]),
        one(FieldPath::HeaderName(2), ViolationCode::Duplicate)
    );
}

#[test]
fn accepts_exactly_20_headers_and_rejects_21() {
    let make = |count: usize| {
        (0..count)
            .map(|index| header(&format!("X-H{index}"), "v"))
            .collect::<Vec<_>>()
    };

    assert!(validate(make(20)).is_ok());
    assert_eq!(
        violations(make(21)),
        one(FieldPath::Headers, ViolationCode::TooMany { max: 20 })
    );
}

#[test]
fn a_401_status_without_www_authenticate_stays_allowed() {
    let config = ResponseConfig::validate(ResponseConfigInput {
        status: 401,
        content_type: "text/plain".to_owned(),
        headers: vec![header("X-Reason", "login required")],
        body: Vec::new(),
        delay_ms: 0,
    })
    .expect("a plain 401 is valid");

    assert_eq!(config.status().unpack(), 401);
}

#[test]
fn validates_every_header_even_when_there_are_too_many() {
    let mut headers: Vec<_> = (0..21)
        .map(|index| header(&format!("X-H{index}"), "v"))
        .collect();
    headers.push(header("Set-Cookie", "a=b"));

    assert_eq!(
        violations(headers),
        [
            Violation {
                field: FieldPath::Headers,
                code: ViolationCode::TooMany { max: 20 }
            },
            Violation {
                field: FieldPath::HeaderName(21),
                code: ViolationCode::Denied
            },
        ]
    );
}

#[test]
fn caps_the_reported_violations_for_hostile_input() {
    let headers: Vec<_> = (0..200_000)
        .map(|_| header("Set-Cookie", "a\r\nb"))
        .collect();

    let started = Instant::now();
    let reported = violations(headers);

    assert_eq!(reported.len(), MAX_VIOLATIONS);
    assert_eq!(MAX_VIOLATIONS, 50);
    assert_eq!(
        reported[0],
        Violation {
            field: FieldPath::Headers,
            code: ViolationCode::TooMany { max: 20 }
        }
    );
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn reports_every_violation_of_one_header() {
    let violations = violations(vec![header("Set\nCookie", "a\r\n\u{e9}")]);

    assert_eq!(
        violations,
        [
            Violation {
                field: FieldPath::HeaderName(0),
                code: ViolationCode::InvalidToken
            },
            Violation {
                field: FieldPath::HeaderValue(0),
                code: ViolationCode::ControlCharacter
            },
            Violation {
                field: FieldPath::HeaderValue(0),
                code: ViolationCode::NonAscii
            },
        ]
    );
}
