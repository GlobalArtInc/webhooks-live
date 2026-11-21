use std::thread::sleep;
use std::time::Duration;

use uuid::Uuid;
use whl_core::capture_id::{CaptureId, CaptureIdError};

const V7: &str = "0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b";

#[test]
fn generate_yields_a_version_7_id_that_parses_back() {
    let id = CaptureId::generate().expect("clock and random source work");

    assert_eq!(id.unpack().get_version_num(), 7);
    assert_eq!(CaptureId::parse(&id.to_string()), Ok(id));
}

#[test]
fn ids_generated_later_sort_after_earlier_ones() {
    let first = CaptureId::generate().expect("clock and random source work");
    sleep(Duration::from_millis(3));
    let second = CaptureId::generate().expect("clock and random source work");

    assert!(first < second);
}

#[test]
fn parse_accepts_lowercase_hyphenated_version_7() {
    let id: CaptureId = V7.parse().expect("valid id");

    assert_eq!(id.to_string(), V7);
    assert_eq!(id.unpack(), Uuid::parse_str(V7).expect("valid uuid"));
}

#[test]
fn parse_rejects_other_versions() {
    assert_eq!(
        CaptureId::parse("67e55044-10b1-426f-9247-bb680e5fe0c8"),
        Err(CaptureIdError::Version)
    );
}

#[test]
fn parse_rejects_a_version_7_uuid_with_a_non_rfc4122_variant() {
    for text in [
        "0193a1c2-9d10-7a3e-cb44-1c2d3e4f5a6b",
        "0193a1c2-9d10-7a3e-0b44-1c2d3e4f5a6b",
    ] {
        assert_eq!(
            CaptureId::parse(text),
            Err(CaptureIdError::Version),
            "{text}"
        );
    }
}

#[test]
fn serde_round_trip_is_a_plain_string_and_deserialisation_is_strict() {
    let id: CaptureId = V7.parse().expect("valid id");

    let json = serde_json::to_string(&id).expect("serialises");

    assert_eq!(json, format!("\"{V7}\""));
    assert_eq!(
        serde_json::from_str::<CaptureId>(&json).expect("deserialises"),
        id
    );
    assert!(serde_json::from_str::<CaptureId>(&format!("\"{}\"", V7.to_uppercase())).is_err());
    assert!(serde_json::from_str::<CaptureId>("\"67e55044-10b1-426f-9247-bb680e5fe0c8\"").is_err());
}

#[test]
fn parse_rejects_non_canonical_forms() {
    assert_eq!(
        CaptureId::parse(&V7.to_uppercase()),
        Err(CaptureIdError::Format)
    );
    assert_eq!(
        CaptureId::parse(&V7.replace('-', "")),
        Err(CaptureIdError::Format)
    );
    assert_eq!(
        CaptureId::parse(&format!("{{{V7}}}")),
        Err(CaptureIdError::Format)
    );
    assert_eq!(
        CaptureId::parse(&format!(" {V7}")),
        Err(CaptureIdError::Format)
    );
    assert_eq!(CaptureId::parse(""), Err(CaptureIdError::Format));
}
