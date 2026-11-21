mod common;

use std::time::Duration;

use common::{WAIT, framed, query, read_frame, start_with};
use hickory_proto::rr::RecordType;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use whl_dns::DnsSettings;

async fn is_closed(stream: &mut TcpStream) -> bool {
    let mut byte = [0_u8; 1];
    matches!(
        tokio::time::timeout(WAIT, stream.read(&mut byte)).await,
        Ok(Ok(0) | Err(_))
    )
}

fn quick_timeouts(settings: &mut DnsSettings) {
    settings.limits.tcp_read_timeout = Duration::from_millis(100);
}

#[tokio::test]
async fn an_oversize_frame_closes_the_connection() {
    let harness = start_with(|_| {}).await;
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    stream
        .write_all(&600_u16.to_be_bytes())
        .await
        .expect("prefix is sent");

    assert!(is_closed(&mut stream).await);
    harness.stop().await;
}

#[tokio::test]
async fn a_frame_shorter_than_a_header_closes_the_connection() {
    let harness = start_with(|_| {}).await;
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    stream
        .write_all(&framed(&[1, 2, 3]))
        .await
        .expect("frame is sent");

    assert!(is_closed(&mut stream).await);
    harness.stop().await;
}

#[tokio::test]
async fn an_idle_connection_is_closed_after_the_read_timeout() {
    let harness = start_with(quick_timeouts).await;
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    assert!(is_closed(&mut stream).await);
    harness.stop().await;
}

#[tokio::test]
async fn a_half_sent_frame_is_closed_after_the_read_timeout() {
    let harness = start_with(quick_timeouts).await;
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");
    let packet = framed(&query(&harness.name(), RecordType::A));

    stream.write_all(&packet[..10]).await.expect("part is sent");

    assert!(is_closed(&mut stream).await);
    harness.stop().await;
}

#[tokio::test]
async fn one_connection_serves_queries_only_up_to_the_per_connection_limit() {
    let harness = start_with(|settings| settings.limits.tcp_max_queries = 3).await;
    let packet = framed(&query(&harness.name(), RecordType::A));
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    for _ in 0..3 {
        stream.write_all(&packet).await.expect("frame is sent");
        assert!(read_frame(&mut stream).await.is_some());
    }
    let _ = stream.write_all(&packet).await;

    assert!(is_closed(&mut stream).await);
    harness.stop().await;
}

#[tokio::test]
async fn connections_beyond_the_limit_are_closed_at_once() {
    let harness = start_with(|settings| settings.limits.max_connections = 2).await;
    let _first = TcpStream::connect(harness.tcp).await.expect("connects");
    let _second = TcpStream::connect(harness.tcp).await.expect("connects");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut third = TcpStream::connect(harness.tcp).await.expect("connects");

    assert!(is_closed(&mut third).await);
    harness.stop().await;
}

#[tokio::test]
async fn a_tcp_flood_is_cut_off_by_the_global_limit() {
    let harness = start_with(|settings| {
        settings.rate_global = "3/m:3".parse().expect("valid rate");
    })
    .await;
    let packet = framed(&query(&harness.name(), RecordType::A));
    let mut stream = TcpStream::connect(harness.tcp).await.expect("connects");

    let mut answered = 0;
    for _ in 0..10 {
        if stream.write_all(&packet).await.is_err() {
            break;
        }
        if read_frame(&mut stream).await.is_some() {
            answered += 1;
        }
    }

    assert!((1..=5).contains(&answered), "{answered} answered");
    harness.stop().await;
}

#[tokio::test]
async fn connections_beyond_the_per_address_limit_are_closed_at_once() {
    let harness = start_with(|settings| settings.limits.tcp_per_address = 2).await;
    let _first = TcpStream::connect(harness.tcp).await.expect("connects");
    let _second = TcpStream::connect(harness.tcp).await.expect("connects");
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut third = TcpStream::connect(harness.tcp).await.expect("connects");

    assert!(is_closed(&mut third).await);
    harness.stop().await;
}

#[tokio::test]
async fn the_per_source_udp_limit_does_not_block_tcp_from_the_same_source() {
    let harness = start_with(|settings| {
        settings.rate_source = "2/m:2".parse().expect("valid rate");
    })
    .await;
    let packet = query(&harness.name(), RecordType::A);
    for _ in 0..10 {
        let _ = common::udp_exchange(harness.udp, &packet).await;
    }

    let reply = common::tcp_exchange(harness.tcp, &packet).await;

    assert!(reply.is_some());
    harness.stop().await;
}
