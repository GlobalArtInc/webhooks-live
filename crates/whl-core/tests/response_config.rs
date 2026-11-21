use std::time::Duration;

use whl_core::header::HeaderInput;
use whl_core::response::{ResponseConfig, ResponseConfigInput, compose_response};
use whl_core::status::{HttpStatus, status_override_from_path};
use whl_core::validation::{FieldPath, Violation, ViolationCode};

fn input() -> ResponseConfigInput {
    ResponseConfigInput {
        status: 200,
        content_type: "text/plain".to_owned(),
        headers: Vec::new(),
        body: Vec::new(),
        delay_ms: 0,
    }
}

fn violations(input: ResponseConfigInput) -> Vec<Violation> {
    ResponseConfig::validate(input)
        .expect_err("input is invalid")
        .violations()
        .to_vec()
}

fn out_of_range(field: FieldPath, min: i64, max: i64) -> Vec<Violation> {
    vec![Violation {
        field,
        code: ViolationCode::OutOfRange { min, max },
    }]
}

#[test]
fn default_config_is_200_text_plain_empty_without_delay() {
    let config = ResponseConfig::default();

    assert_eq!(config.status().unpack(), 200);
    assert_eq!(config.content_type().unpack(), "text/plain");
    assert!(config.headers().is_empty());
    assert!(config.body().is_empty());
    assert_eq!(config.delay(), Duration::ZERO);
    assert_eq!(ResponseConfig::validate(input()), Ok(config));
}

#[test]
fn status_accepts_100_and_599_and_rejects_outside() {
    for status in [100, 101, 599] {
        let config = ResponseConfig::validate(ResponseConfigInput { status, ..input() })
            .expect("status in range");

        assert_eq!(i64::from(config.status().unpack()), status);
    }
    for status in [99, 600, 0, -1, 65_736, i64::MAX, i64::MIN] {
        assert_eq!(
            violations(ResponseConfigInput { status, ..input() }),
            out_of_range(FieldPath::Status, 100, 599),
            "status {status}"
        );
    }
}

#[test]
fn delay_accepts_0_and_10000_ms_and_rejects_outside() {
    for delay_ms in [0, 1, 10_000] {
        let config = ResponseConfig::validate(ResponseConfigInput {
            delay_ms,
            ..input()
        })
        .expect("delay in range");

        assert_eq!(
            config.delay(),
            Duration::from_millis(delay_ms.unsigned_abs())
        );
    }
    for delay_ms in [-1, 10_001, i64::MAX, i64::MIN] {
        assert_eq!(
            violations(ResponseConfigInput {
                delay_ms,
                ..input()
            }),
            out_of_range(FieldPath::DelayMs, 0, 10_000),
            "delay {delay_ms}"
        );
    }
}

#[test]
fn body_accepts_exactly_64_kib_and_rejects_one_more_byte() {
    let at_limit = ResponseConfigInput {
        body: vec![b'a'; 65_536],
        ..input()
    };
    let over_limit = ResponseConfigInput {
        body: vec![b'a'; 65_537],
        ..input()
    };

    assert!(ResponseConfig::validate(at_limit).is_ok());
    assert_eq!(
        violations(over_limit),
        [Violation {
            field: FieldPath::Body,
            code: ViolationCode::TooLong { max: 65_536 }
        }]
    );
}

#[test]
fn content_type_violation_carries_the_content_type_path() {
    assert_eq!(
        violations(ResponseConfigInput {
            content_type: "nonsense".to_owned(),
            ..input()
        }),
        [Violation {
            field: FieldPath::ContentType,
            code: ViolationCode::InvalidMediaType
        }]
    );
}

#[test]
fn validation_error_lists_every_violation_at_once() {
    let error = ResponseConfig::validate(ResponseConfigInput {
        status: 700,
        content_type: "bad\r\n".to_owned(),
        headers: vec![
            HeaderInput {
                name: "Set-Cookie".to_owned(),
                value: "a=b".to_owned(),
            },
            HeaderInput {
                name: "X-A".to_owned(),
                value: "x\ny".to_owned(),
            },
        ],
        body: vec![0; 65_537],
        delay_ms: 20_000,
    })
    .expect_err("everything is invalid");

    assert_eq!(
        error.violations(),
        [
            Violation {
                field: FieldPath::Status,
                code: ViolationCode::OutOfRange { min: 100, max: 599 }
            },
            Violation {
                field: FieldPath::ContentType,
                code: ViolationCode::ControlCharacter
            },
            Violation {
                field: FieldPath::HeaderName(0),
                code: ViolationCode::Denied
            },
            Violation {
                field: FieldPath::HeaderValue(1),
                code: ViolationCode::ControlCharacter
            },
            Violation {
                field: FieldPath::Body,
                code: ViolationCode::TooLong { max: 65_536 }
            },
            Violation {
                field: FieldPath::DelayMs,
                code: ViolationCode::OutOfRange {
                    min: 0,
                    max: 10_000
                }
            },
        ]
    );
}

#[test]
fn violation_exposes_a_field_path_and_a_machine_readable_code() {
    let violation = Violation {
        field: FieldPath::HeaderValue(3),
        code: ViolationCode::ControlCharacter,
    };

    assert_eq!(violation.field.to_string(), "response.headers[3].value");
    assert_eq!(violation.code.as_str(), "control_character");
    assert_eq!(
        violation.to_string(),
        "response.headers[3].value must not contain control characters"
    );
}

#[test]
fn composition_without_override_uses_the_configured_response() {
    let config = ResponseConfig::validate(ResponseConfigInput {
        status: 202,
        content_type: "application/json".to_owned(),
        headers: vec![HeaderInput {
            name: "X-Custom".to_owned(),
            value: "1".to_owned(),
        }],
        ..input()
    })
    .expect("valid config");

    let composed = compose_response(&config, None);

    assert_eq!(composed.status.unpack(), 202);
    assert_eq!(composed.content_type.unpack(), "application/json");
    assert_eq!(composed.headers, config.headers());
}

#[test]
fn composition_honours_the_status_override_and_keeps_the_rest() {
    let config = ResponseConfig::validate(ResponseConfigInput {
        status: 202,
        content_type: "application/json".to_owned(),
        ..input()
    })
    .expect("valid config");

    let composed = compose_response(&config, status_override_from_path("/404/orders"));

    assert_eq!(composed.status, HttpStatus::new(404).expect("valid status"));
    assert_eq!(composed.content_type.unpack(), "application/json");
}
