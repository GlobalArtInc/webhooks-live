use std::net::SocketAddr;

use whl_core::proxy_protocol::{
    MAX_PAYLOAD_LEN, PREFIX_LEN, ProxiedAddresses, ProxyError, ProxyHeader, SIGNATURE, header_len,
    parse,
};

fn header(command: u8, family: u8, block: &[u8]) -> Vec<u8> {
    let mut bytes = SIGNATURE.to_vec();
    bytes.push(command);
    bytes.push(family);
    bytes.extend_from_slice(
        &u16::try_from(block.len())
            .expect("small block")
            .to_be_bytes(),
    );
    bytes.extend_from_slice(block);
    bytes
}

fn v4_block() -> Vec<u8> {
    let mut block = vec![203, 0, 113, 9, 10, 0, 0, 1];
    block.extend_from_slice(&4000_u16.to_be_bytes());
    block.extend_from_slice(&53_u16.to_be_bytes());
    block
}

fn socket(text: &str) -> SocketAddr {
    text.parse().expect("literal address")
}

#[test]
fn an_ipv4_header_yields_both_addresses_and_its_length() {
    let mut bytes = header(0x21, 0x12, &v4_block());
    bytes.extend_from_slice(b"payload");

    let parsed = parse(&bytes).expect("the header parses");

    assert_eq!(parsed.consumed, PREFIX_LEN + 12);
    assert_eq!(
        parsed.header,
        ProxyHeader::Proxied(ProxiedAddresses {
            source: socket("203.0.113.9:4000"),
            destination: socket("10.0.0.1:53"),
        })
    );
}

#[test]
fn an_ipv6_header_yields_both_addresses() {
    let mut block = Vec::new();
    block.extend_from_slice(
        &"2001:db8::9"
            .parse::<std::net::Ipv6Addr>()
            .expect("literal")
            .octets(),
    );
    block.extend_from_slice(
        &"2001:db8::1"
            .parse::<std::net::Ipv6Addr>()
            .expect("literal")
            .octets(),
    );
    block.extend_from_slice(&4000_u16.to_be_bytes());
    block.extend_from_slice(&53_u16.to_be_bytes());

    let parsed = parse(&header(0x21, 0x21, &block)).expect("the header parses");

    assert_eq!(
        parsed.header,
        ProxyHeader::Proxied(ProxiedAddresses {
            source: socket("[2001:db8::9]:4000"),
            destination: socket("[2001:db8::1]:53"),
        })
    );
}

#[test]
fn trailing_tlv_bytes_are_consumed_with_the_header() {
    let mut block = v4_block();
    block.extend_from_slice(&[0x04, 0x00, 0x02, 0xaa, 0xbb]);

    let parsed = parse(&header(0x21, 0x12, &block)).expect("the header parses");

    assert_eq!(parsed.consumed, PREFIX_LEN + block.len());
}

#[test]
fn the_local_command_parses_without_addresses() {
    let parsed = parse(&header(0x20, 0x00, &[])).expect("the header parses");

    assert_eq!(parsed.header, ProxyHeader::Local);
}

#[test]
fn a_wrong_signature_is_refused() {
    let mut bytes = header(0x21, 0x12, &v4_block());
    bytes[3] ^= 0xff;

    assert_eq!(parse(&bytes), Err(ProxyError::Signature));
}

#[test]
fn a_version_other_than_two_is_refused() {
    assert_eq!(
        parse(&header(0x11, 0x12, &v4_block())),
        Err(ProxyError::Version(1))
    );
}

#[test]
fn an_unknown_command_is_refused() {
    assert_eq!(
        parse(&header(0x22, 0x12, &v4_block())),
        Err(ProxyError::Command(2))
    );
}

#[test]
fn a_header_shorter_than_its_length_field_is_refused() {
    let bytes = header(0x21, 0x12, &v4_block());

    assert_eq!(parse(&bytes[..bytes.len() - 1]), Err(ProxyError::Truncated));
    assert_eq!(parse(&bytes[..10]), Err(ProxyError::Truncated));
}

#[test]
fn a_declared_length_above_the_cap_is_refused_from_the_prefix_alone() {
    let mut prefix = SIGNATURE.to_vec();
    prefix.extend_from_slice(&[0x21, 0x12]);
    prefix.extend_from_slice(
        &u16::try_from(MAX_PAYLOAD_LEN + 1)
            .expect("fits")
            .to_be_bytes(),
    );

    assert_eq!(
        header_len(&prefix),
        Err(ProxyError::TooLong {
            declared: MAX_PAYLOAD_LEN + 1
        })
    );
}

#[test]
fn an_address_block_shorter_than_the_family_needs_is_refused() {
    assert_eq!(
        parse(&header(0x21, 0x12, &v4_block()[..8])),
        Err(ProxyError::AddressBlock)
    );
}

#[test]
fn the_unix_family_is_refused_for_the_proxy_command() {
    assert_eq!(
        parse(&header(0x21, 0x31, &[0_u8; 216])),
        Err(ProxyError::Unsupported(0x31))
    );
}

#[test]
fn an_unspecified_family_under_the_proxy_command_carries_no_addresses() {
    let parsed = parse(&header(0x21, 0x00, &[])).expect("the header parses");

    assert_eq!(parsed.header, ProxyHeader::Local);
}
