use std::net::IpAddr;

use whl_core::sender_allow::{
    MAX_SENDER_RANGES, SenderAllowList, SenderRange, SenderRangeError, TooManyRanges,
};

fn range(text: &str) -> SenderRange {
    SenderRange::parse(text).expect("the range parses")
}

fn address(text: &str) -> IpAddr {
    text.parse().expect("the address parses")
}

fn list(texts: &[&str]) -> SenderAllowList {
    SenderAllowList::new(texts.iter().map(|text| range(text)).collect()).expect("the list fits")
}

#[test]
fn a_bare_address_becomes_a_single_host_range() {
    assert_eq!(range("203.0.113.7").to_string(), "203.0.113.7/32");
    assert_eq!(range("2001:db8::1").to_string(), "2001:db8::1/128");
    assert_eq!(range("203.0.113.0/24").to_string(), "203.0.113.0/24");
}

#[test]
fn text_that_is_not_a_range_is_refused() {
    for text in [
        "",
        "not-an-ip",
        "203.0.113.0/33",
        "203.0.113.0/",
        " 203.0.113.7",
    ] {
        assert_eq!(
            SenderRange::parse(text),
            Err(SenderRangeError::Unparsable),
            "{text:?}"
        );
    }
    assert_eq!(
        SenderRange::parse(&"1".repeat(65)),
        Err(SenderRangeError::Unparsable)
    );
}

#[test]
fn a_range_with_bits_set_beyond_its_prefix_is_refused() {
    assert_eq!(
        SenderRange::parse("203.0.113.7/24"),
        Err(SenderRangeError::HostBits)
    );
    assert_eq!(
        SenderRange::parse("2001:db8::1/32"),
        Err(SenderRangeError::HostBits)
    );
}

#[test]
fn an_ipv4_mapped_sender_matches_an_ipv4_range_and_a_mapped_range_reads_as_ipv4() {
    let allowed = list(&["203.0.113.0/24"]);
    assert!(allowed.permits(address("::ffff:203.0.113.9")));
    assert!(!allowed.permits(address("::ffff:203.0.114.9")));
    assert_eq!(
        range("::ffff:203.0.113.0/120").to_string(),
        "203.0.113.0/24"
    );
    assert_eq!(range("::ffff:203.0.113.7").to_string(), "203.0.113.7/32");
}

#[test]
fn an_empty_list_permits_everyone_and_a_filled_one_only_its_ranges() {
    assert!(SenderAllowList::default().permits(address("198.51.100.1")));

    let allowed = list(&["203.0.113.0/24", "2001:db8::/32"]);
    assert!(allowed.permits(address("203.0.113.200")));
    assert!(allowed.permits(address("2001:db8:1::5")));
    assert!(!allowed.permits(address("198.51.100.1")));
    assert!(!allowed.permits(address("2001:db9::1")));
}

#[test]
fn the_list_holds_at_most_twenty_distinct_ranges() {
    let texts: Vec<String> = (0..=MAX_SENDER_RANGES)
        .map(|index| format!("203.0.113.{index}"))
        .collect();
    let ranges: Vec<SenderRange> = texts.iter().map(|text| range(text)).collect();

    assert_eq!(SenderAllowList::new(ranges.clone()), Err(TooManyRanges));
    let mut with_duplicate = ranges[..MAX_SENDER_RANGES].to_vec();
    with_duplicate.push(ranges[0]);
    let kept = SenderAllowList::new(with_duplicate).expect("a duplicate does not count twice");
    assert_eq!(kept.ranges().len(), MAX_SENDER_RANGES);
}
