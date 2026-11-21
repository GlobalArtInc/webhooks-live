mod common;

use common::{
    WAIT, decode, framed, proxy_v2, query, read_frame, receive, start_with, udp_exchange,
};
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::RecordType;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use whl_core::proxy_protocol::SIGNATURE;
use whl_dns::DnsSettings;

fn trust_loopback(settings: &mut DnsSettings) {
    settings.trusted_proxies =
        whl_core::client_ip::TrustedProxies::new(vec!["127.0.0.0/8".parse().expect("literal")]);
}

fn trust_other_network(settings: &mut DnsSettings) {
    settings.trusted_proxies =
        whl_core::client_ip::TrustedProxies::new(vec!["10.0.0.0/8".parse().expect("literal")]);
}

fn prefixed(header: &[u8], packet: &[u8]) -> Vec<u8> {
    let mut bytes = header.to_vec();
    bytes.extend_from_slice(packet);
    bytes
}

#[tokio::test]
async fn a_udp_proxy_header_from_a_trusted_peer_sets_the_resolver_address() {
    let harness = start_with(trust_loopback).await;
    let packet = query(&harness.name(), RecordType::A);

    for source in ["203.0.113.9:4000", "[2001:db8::9]:4000"] {
        let header = proxy_v2(source.parse().expect("literal address"), true);
        let reply = udp_exchange(harness.udp, &prefixed(&header, &packet))
            .await
            .expect("the server answers");
        assert_eq!(decode(&reply).metadata.response_code, ResponseCode::NoError);
    }

    let rows = harness.rows().await;
    assert_eq!(rows[0].client_ip, "203.0.113.9/32");
    assert_eq!(rows[1].client_ip, "2001:db8::9/128");
    harness.stop().await;
}

#[tokio::test]
async fn a_datagram_without_a_header_from_a_trusted_peer_is_dropped() {
    let harness = start_with(trust_loopback).await;

    let reply = udp_exchange(harness.udp, &query(&harness.name(), RecordType::A)).await;

    assert!(reply.is_none());
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn corrupt_proxy_headers_are_dropped() {
    let harness = start_with(trust_loopback).await;
    let packet = query(&harness.name(), RecordType::A);
    let source = "203.0.113.9:4000".parse().expect("literal address");
    let mut bad_version = proxy_v2(source, true);
    bad_version[12] = 0x11;
    let mut oversize = SIGNATURE.to_vec();
    oversize.extend_from_slice(&[0x21, 0x12, 0xff, 0xff]);
    let truncated = proxy_v2(source, true)[..20].to_vec();

    let datagrams = [
        prefixed(&bad_version, &packet),
        prefixed(&oversize, &packet),
        truncated,
    ];
    for datagram in datagrams {
        assert!(udp_exchange(harness.udp, &datagram).await.is_none());
    }
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn a_proxy_header_from_an_untrusted_peer_is_not_honoured() {
    let harness = start_with(trust_other_network).await;
    let header = proxy_v2("203.0.113.9:4000".parse().expect("literal address"), true);

    let reply = udp_exchange(
        harness.udp,
        &prefixed(&header, &query(&harness.name(), RecordType::A)),
    )
    .await;

    if let Some(bytes) = reply {
        assert_eq!(decode(&bytes).metadata.response_code, ResponseCode::NotImp);
    }
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn a_tcp_proxy_header_from_a_trusted_peer_sets_the_resolver_address() {
    let harness = start_with(trust_loopback).await;
    let header = proxy_v2("203.0.113.10:5000".parse().expect("literal address"), false);
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    stream.write_all(&header).await.expect("header is sent");
    stream
        .write_all(&framed(&query(&harness.name(), RecordType::A)))
        .await
        .expect("frame is sent");
    let reply = read_frame(&mut stream).await.expect("the server answers");

    assert_eq!(decode(&reply).metadata.response_code, ResponseCode::NoError);
    assert_eq!(harness.rows().await[0].client_ip, "203.0.113.10/32");
    harness.stop().await;
}

#[tokio::test]
async fn a_tcp_connection_from_a_trusted_peer_without_a_header_is_closed() {
    let harness = start_with(trust_loopback).await;
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    stream
        .write_all(&framed(&query(&harness.name(), RecordType::A)))
        .await
        .expect("frame is sent");
    let mut rest = Vec::new();
    let closed = tokio::time::timeout(WAIT, stream.read_to_end(&mut rest)).await;

    assert!(closed.is_ok(), "the connection stayed open");
    assert!(rest.is_empty());
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn a_reply_goes_back_to_the_sender_of_the_datagram_not_the_header_address() {
    let harness = start_with(trust_loopback).await;
    let header = proxy_v2("203.0.113.9:4000".parse().expect("literal address"), true);
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client binds");

    socket
        .send_to(
            &prefixed(&header, &query(&harness.name(), RecordType::A)),
            harness.udp,
        )
        .await
        .expect("datagram is sent");

    assert!(receive(&socket).await.is_some());
    harness.stop().await;
}

#[tokio::test]
async fn a_local_proxy_header_is_accepted_and_the_peer_address_is_the_resolver() {
    let harness = start_with(trust_loopback).await;
    let mut local = proxy_v2("203.0.113.9:4000".parse().expect("literal address"), true);
    local[12] = 0x20;

    let reply = udp_exchange(
        harness.udp,
        &prefixed(&local, &query(&harness.name(), RecordType::A)),
    )
    .await
    .expect("the server answers");

    assert_eq!(decode(&reply).metadata.response_code, ResponseCode::NoError);
    assert_eq!(harness.rows().await[0].client_ip, "127.0.0.1/32");
    harness.stop().await;
}

#[tokio::test]
async fn a_local_proxy_header_on_tcp_is_accepted_and_the_peer_address_is_the_resolver() {
    let harness = start_with(trust_loopback).await;
    let mut local = proxy_v2("203.0.113.9:4000".parse().expect("literal address"), false);
    local[12] = 0x20;
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    stream.write_all(&local).await.expect("header is sent");
    stream
        .write_all(&framed(&query(&harness.name(), RecordType::A)))
        .await
        .expect("frame is sent");
    let reply = read_frame(&mut stream).await.expect("the server answers");

    assert_eq!(decode(&reply).metadata.response_code, ResponseCode::NoError);
    assert_eq!(harness.rows().await[0].client_ip, "127.0.0.1/32");
    harness.stop().await;
}
