use std::net::SocketAddr;

use whl_core::proxy_protocol::{
    MAX_PAYLOAD_LEN, PREFIX_LEN, Prefix, ProxyError, ProxyHeader, SIGNATURE,
};

fn header(command: u8, family_transport: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = SIGNATURE.to_vec();
    bytes.push(0x20 | command);
    bytes.push(family_transport);
    bytes.extend_from_slice(&u16::try_from(payload.len()).expect("length").to_be_bytes());
    bytes.extend_from_slice(payload);
    bytes
}

fn inet_payload(source: [u8; 4], destination: [u8; 4], ports: (u16, u16)) -> Vec<u8> {
    let mut payload = source.to_vec();
    payload.extend_from_slice(&destination);
    payload.extend_from_slice(&ports.0.to_be_bytes());
    payload.extend_from_slice(&ports.1.to_be_bytes());
    payload
}

#[test]
fn an_ipv4_stream_header_yields_both_addresses() {
    let payload = inet_payload([203, 0, 113, 9], [10, 0, 0, 5], (51000, 25));
    let bytes = header(1, 0x11, &payload);

    let (parsed, consumed) = ProxyHeader::parse(&bytes).expect("valid header");

    assert_eq!(consumed, bytes.len());
    assert_eq!(
        parsed,
        ProxyHeader::Proxied {
            source: "203.0.113.9:51000".parse::<SocketAddr>().expect("address"),
            destination: "10.0.0.5:25".parse::<SocketAddr>().expect("address"),
        }
    );
}

#[test]
fn an_ipv6_stream_header_yields_both_addresses() {
    let mut payload = "2001:db8::1"
        .parse::<std::net::Ipv6Addr>()
        .expect("ip")
        .octets()
        .to_vec();
    payload.extend_from_slice(
        &"2001:db8::2"
            .parse::<std::net::Ipv6Addr>()
            .expect("ip")
            .octets(),
    );
    payload.extend_from_slice(&40000_u16.to_be_bytes());
    payload.extend_from_slice(&25_u16.to_be_bytes());
    let bytes = header(1, 0x21, &payload);

    let (parsed, _) = ProxyHeader::parse(&bytes).expect("valid header");

    assert_eq!(
        parsed,
        ProxyHeader::Proxied {
            source: "[2001:db8::1]:40000".parse().expect("address"),
            destination: "[2001:db8::2]:25".parse().expect("address"),
        }
    );
}

#[test]
fn trailing_tlv_bytes_are_skipped_and_counted() {
    let mut payload = inet_payload([192, 0, 2, 1], [10, 0, 0, 5], (1, 2));
    payload.extend_from_slice(&[0x04, 0x00, 0x02, 0xAA, 0xBB]);
    let mut bytes = header(1, 0x11, &payload);
    bytes.extend_from_slice(b"EHLO");

    let (_, consumed) = ProxyHeader::parse(&bytes).expect("valid header");

    assert_eq!(consumed, PREFIX_LEN + payload.len());
}

#[test]
fn the_local_command_carries_no_address() {
    let bytes = header(0, 0x00, &[]);

    let (parsed, _) = ProxyHeader::parse(&bytes).expect("valid header");

    assert_eq!(parsed, ProxyHeader::Local);
}

#[test]
fn a_wrong_signature_is_refused() {
    let mut bytes = header(1, 0x11, &inet_payload([1, 1, 1, 1], [2, 2, 2, 2], (1, 1)));
    bytes[3] ^= 0xFF;

    assert_eq!(ProxyHeader::parse(&bytes), Err(ProxyError::BadSignature));
}

#[test]
fn the_text_form_of_version_one_is_refused() {
    let text = b"PROXY TCP4 203.0.113.9 10.0.0.5 51000 25\r\n";

    assert_eq!(ProxyHeader::parse(text), Err(ProxyError::BadSignature));
}

#[test]
fn another_version_is_refused() {
    let mut bytes = header(1, 0x11, &inet_payload([1, 1, 1, 1], [2, 2, 2, 2], (1, 1)));
    bytes[12] = 0x31;

    assert_eq!(
        ProxyHeader::parse(&bytes),
        Err(ProxyError::UnsupportedVersion(3))
    );
}

#[test]
fn an_unknown_command_is_refused() {
    let bytes = header(2, 0x11, &inet_payload([1, 1, 1, 1], [2, 2, 2, 2], (1, 1)));

    assert_eq!(
        ProxyHeader::parse(&bytes),
        Err(ProxyError::UnsupportedCommand(2))
    );
}

#[test]
fn datagram_and_unix_transports_are_refused() {
    let payload = inet_payload([1, 1, 1, 1], [2, 2, 2, 2], (1, 1));

    assert_eq!(
        ProxyHeader::parse(&header(1, 0x12, &payload)),
        Err(ProxyError::UnsupportedTransport)
    );
    assert_eq!(
        ProxyHeader::parse(&header(1, 0x31, &[0; 216])),
        Err(ProxyError::UnsupportedTransport)
    );
}

#[test]
fn a_payload_shorter_than_the_address_block_is_refused() {
    let bytes = header(1, 0x11, &[0; 8]);

    assert_eq!(
        ProxyHeader::parse(&bytes),
        Err(ProxyError::PayloadTooShort { needed: 12, got: 8 })
    );
}

#[test]
fn a_declared_payload_above_the_limit_is_refused_from_the_prefix_alone() {
    let mut bytes = header(1, 0x11, &[]);
    let declared = u16::try_from(MAX_PAYLOAD_LEN + 1).expect("length");
    bytes[14..16].copy_from_slice(&declared.to_be_bytes());
    let prefix: [u8; PREFIX_LEN] = bytes[..PREFIX_LEN].try_into().expect("prefix");

    assert_eq!(
        Prefix::parse(&prefix),
        Err(ProxyError::PayloadTooLarge(MAX_PAYLOAD_LEN + 1))
    );
}

#[test]
fn a_header_cut_short_is_reported() {
    let bytes = header(1, 0x11, &inet_payload([1, 1, 1, 1], [2, 2, 2, 2], (1, 1)));

    assert_eq!(ProxyHeader::parse(&bytes[..10]), Err(ProxyError::Truncated));
    assert_eq!(
        ProxyHeader::parse(&bytes[..bytes.len() - 1]),
        Err(ProxyError::Truncated)
    );
}

#[test]
fn an_unspecified_source_address_is_refused() {
    let bytes = header(1, 0x11, &inet_payload([0, 0, 0, 0], [2, 2, 2, 2], (1, 1)));

    assert_eq!(ProxyHeader::parse(&bytes), Err(ProxyError::InvalidAddress));
}
