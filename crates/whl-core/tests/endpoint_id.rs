use std::collections::HashSet;

use whl_core::endpoint_id::{ENDPOINT_ID_ALPHABET, ENDPOINT_ID_LEN, EndpointId, EndpointIdError};

const VALID: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

#[test]
fn generate_yields_24_characters_from_the_alphabet() {
    let id = EndpointId::generate().expect("the OS random source works");

    assert_eq!(id.unpack().len(), ENDPOINT_ID_LEN);
    assert!(
        id.unpack()
            .bytes()
            .all(|b| ENDPOINT_ID_ALPHABET.contains(&b))
    );
    assert_eq!(EndpointId::parse(id.unpack()), Ok(id));
}

#[test]
fn generate_covers_every_position_with_the_whole_alphabet() {
    const SAMPLES: usize = 2000;
    const MIN_DISTINCT: usize = 28;

    let ids: Vec<_> = (0..SAMPLES)
        .map(|_| EndpointId::generate().expect("the OS random source works"))
        .collect();

    for position in 0..ENDPOINT_ID_LEN {
        let distinct: HashSet<u8> = ids
            .iter()
            .map(|id| id.unpack().as_bytes()[position])
            .collect();

        assert!(
            distinct.len() >= MIN_DISTINCT,
            "position {position} saw only {} distinct characters",
            distinct.len()
        );
    }
}

#[test]
fn two_generated_ids_differ() {
    let first = EndpointId::generate().expect("the OS random source works");
    let second = EndpointId::generate().expect("the OS random source works");

    assert_ne!(first, second);
}

#[test]
fn parse_accepts_a_valid_id_and_round_trips_through_display() {
    let id: EndpointId = VALID.parse().expect("valid id");

    assert_eq!(id.unpack(), VALID);
    assert_eq!(id.to_string(), VALID);
}

#[test]
fn parse_rejects_wrong_length() {
    assert_eq!(
        EndpointId::parse(&VALID[..23]),
        Err(EndpointIdError::Length { actual: 23 })
    );
    assert_eq!(
        EndpointId::parse(&format!("{VALID}0")),
        Err(EndpointIdError::Length { actual: 25 })
    );
}

#[test]
fn parse_rejects_empty_string() {
    assert_eq!(
        EndpointId::parse(""),
        Err(EndpointIdError::Length { actual: 0 })
    );
}

#[test]
fn parse_rejects_uppercase_without_case_folding() {
    assert_eq!(
        EndpointId::parse(&VALID.to_uppercase()),
        Err(EndpointIdError::Character { index: 1 })
    );
}

#[test]
fn parse_rejects_each_excluded_letter_without_normalising_lookalikes() {
    for letter in ['i', 'l', 'o', 'u'] {
        let text = format!("{}{letter}{}", &VALID[..5], &VALID[6..]);

        assert_eq!(
            EndpointId::parse(&text),
            Err(EndpointIdError::Character { index: 5 }),
            "letter {letter}"
        );
    }
}

#[test]
fn parse_rejects_whitespace_without_trimming() {
    assert!(EndpointId::parse(&format!(" {}", &VALID[..23])).is_err());
    assert!(EndpointId::parse(&format!("{} ", &VALID[..23])).is_err());
    assert!(EndpointId::parse(&format!("{VALID}\n")).is_err());
}

#[test]
fn parse_rejects_unicode_lookalikes() {
    let fullwidth = format!("{}\u{ff10}{}", &VALID[..5], &VALID[6..]);
    let cyrillic_a = format!("{}\u{0430}", &VALID[..21]);

    assert!(EndpointId::parse(&fullwidth).is_err());
    assert!(EndpointId::parse(&cyrillic_a).is_err());
}

#[test]
fn serde_round_trip_is_a_plain_string_and_deserialisation_is_strict() {
    let id: EndpointId = VALID.parse().expect("valid id");

    assert_eq!(
        serde_json::to_string(&id).expect("serialises"),
        format!("\"{VALID}\"")
    );
    assert!(serde_json::from_str::<EndpointId>("\"ABC\"").is_err());
}
