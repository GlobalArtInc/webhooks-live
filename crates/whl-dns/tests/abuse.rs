mod common;

use std::time::Duration;

use common::{Harness, WAIT, ZONE, decode, query, receive, start, start_with, udp_exchange};
use hickory_proto::op::ResponseCode;
use hickory_proto::rr::RecordType;
use tokio::net::UdpSocket;
use whl_core::endpoint_id::EndpointId;
use whl_core::rate::RateSpec;
use whl_dns::DnsSettings;

const SHORT_WAIT: Duration = Duration::from_millis(40);

fn rate(text: &str) -> RateSpec {
    text.parse().expect("test rate is valid")
}

fn lenient_sources(settings: &mut DnsSettings) {
    settings.rate_source = rate("100000/s:100000");
}

fn unknown_id_name() -> String {
    format!(
        "{}.{ZONE}",
        EndpointId::generate().expect("entropy").unpack()
    )
}

async fn exchange_quickly(harness: &Harness, packet: &[u8]) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client binds");
    socket
        .send_to(packet, harness.udp)
        .await
        .expect("datagram is sent");
    let mut buffer = vec![0_u8; 4096];
    match tokio::time::timeout(SHORT_WAIT, socket.recv_from(&mut buffer)).await {
        Ok(Ok((length, _))) => Some(buffer[..length].to_vec()),
        _ => None,
    }
}

fn corrupt_packets(valid: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
    let mut label_too_long = valid.to_vec();
    label_too_long[12] = 64;
    let mut pointer_loop = valid[..12].to_vec();
    pointer_loop.extend_from_slice(&[0xc0, 0x0c, 0, 1, 0, 1]);
    let mut trailing = valid.to_vec();
    trailing.extend_from_slice(&[0xde, 0xad]);
    let mut many_questions = valid[..12].to_vec();
    many_questions[4] = 0xff;
    many_questions[5] = 0xff;
    let mut response = valid.to_vec();
    response[2] |= 0x80;
    vec![
        ("a bare header with no question", valid[..12].to_vec()),
        ("a question cut in half", valid[..valid.len() / 2].to_vec()),
        ("a label longer than 63 bytes", label_too_long),
        ("a compression pointer to itself", pointer_loop),
        ("trailing bytes after the message", trailing),
        ("a header claiming 65535 questions", many_questions),
        ("a packet marked as a response", response),
    ]
}

#[tokio::test]
async fn malformed_packets_are_answered_with_a_header_only_formerr_or_dropped() {
    let harness = start().await;
    let valid = query(&harness.name(), RecordType::A);

    for (what, packet) in corrupt_packets(&valid) {
        let reply = udp_exchange(harness.udp, &packet).await;
        match reply {
            Some(bytes) => {
                assert_eq!(bytes.len(), 12, "{what}");
                assert_eq!(decode(&bytes).metadata.response_code, ResponseCode::FormErr);
            }
            None => assert_eq!(what, "a packet marked as a response"),
        }
    }
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn empty_short_and_oversize_datagrams_get_no_answer() {
    let harness = start().await;
    let valid = query(&harness.name(), RecordType::A);
    let mut oversize = valid.clone();
    oversize.resize(600, 0);

    for packet in [Vec::new(), vec![0_u8; 5], oversize] {
        assert!(udp_exchange(harness.udp, &packet).await.is_none());
    }
    assert!(udp_exchange(harness.udp, &valid).await.is_some());
    harness.stop().await;
}

#[tokio::test]
async fn random_bytes_never_stop_the_server_or_grow_into_a_larger_reply() {
    let harness = start_with(lenient_sources).await;
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for _ in 0..120 {
        let length = usize::try_from(next() % 600).expect("small");
        let packet: Vec<u8> = (0..length).map(|_| (next() % 256) as u8).collect();
        if let Some(reply) = exchange_quickly(&harness, &packet).await {
            assert!(reply.len() <= packet.len().max(12), "{} bytes", reply.len());
        }
    }

    let reply = udp_exchange(harness.udp, &query(&harness.name(), RecordType::A)).await;
    assert!(reply.is_some());
    harness.stop().await;
}

#[tokio::test]
async fn unknown_ids_are_looked_up_once_then_served_from_the_negative_cache() {
    let harness = start_with(lenient_sources).await;
    let name = unknown_id_name();

    for _ in 0..40 {
        let reply = udp_exchange(harness.udp, &query(&name, RecordType::A))
            .await
            .expect("the server answers");
        assert_eq!(
            decode(&reply).metadata.response_code,
            ResponseCode::NXDomain
        );
    }

    assert_eq!(harness.handle.responder().database_lookups(), 1);
    harness.stop().await;
}

#[tokio::test]
async fn names_without_an_endpoint_label_never_reach_the_database() {
    let harness = start_with(lenient_sources).await;

    for index in 0..30 {
        let name = format!("random-{index}.{ZONE}");
        udp_exchange(harness.udp, &query(&name, RecordType::A))
            .await
            .expect("the server answers");
    }

    assert_eq!(harness.handle.responder().database_lookups(), 0);
    harness.stop().await;
}

#[tokio::test]
async fn database_lookups_are_bounded_when_every_name_is_a_new_valid_id() {
    let harness = start_with(|settings| {
        lenient_sources(settings);
        settings.limits.lookup_rate = rate("3/m:3");
    })
    .await;

    let mut answered = 0;
    for _ in 0..40 {
        if exchange_quickly(&harness, &query(&unknown_id_name(), RecordType::A))
            .await
            .is_some()
        {
            answered += 1;
        }
    }

    assert!(harness.handle.responder().database_lookups() <= 5);
    assert!(answered <= 5, "{answered} answered");
    harness.stop().await;
}

#[tokio::test]
async fn a_disabled_endpoint_answers_nxdomain_and_is_not_stored() {
    let harness = start().await;
    harness
        .store
        .set_dns_enabled(&harness.endpoint, false)
        .await
        .expect("the flag is written");

    let reply = udp_exchange(harness.udp, &query(&harness.name(), RecordType::A))
        .await
        .expect("the server answers");

    assert_eq!(
        decode(&reply).metadata.response_code,
        ResponseCode::NXDomain
    );
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn a_flood_from_one_source_is_cut_to_the_limit_and_slips_small_truncated_answers() {
    let harness = start_with(|settings| settings.rate_source = rate("5/s:5")).await;
    let packet = query(&harness.name(), RecordType::A);
    let socket = UdpSocket::bind("127.0.0.1:0").await.expect("client binds");

    for _ in 0..100 {
        socket
            .send_to(&packet, harness.udp)
            .await
            .expect("datagram is sent");
    }
    let mut full = 0;
    let mut truncated = 0;
    while let Some(reply) = receive(&socket).await {
        let message = decode(&reply);
        if message.metadata.truncation {
            truncated += 1;
            assert!(reply.len() <= packet.len(), "{} bytes", reply.len());
            assert!(message.answers.is_empty());
        } else {
            full += 1;
        }
    }

    assert!((1..=7).contains(&full), "{full} full answers");
    assert!(truncated >= 1, "no truncated answer slipped through");
    assert!(full + truncated < 100, "{} replies", full + truncated);
    assert!(harness.rows().await.len() <= 7);
    harness.stop().await;
}

#[tokio::test]
async fn the_global_limit_caps_responses_across_many_sources() {
    let harness = start_with(|settings| {
        lenient_sources(settings);
        settings.rate_global = rate("5/m:5");
    })
    .await;
    let packet = query(&harness.name(), RecordType::A);

    let mut replies = 0;
    for _ in 0..40 {
        if exchange_quickly(&harness, &packet).await.is_some() {
            replies += 1;
        }
    }

    assert!((1..=7).contains(&replies), "{replies} replies");
    harness.stop().await;
}

#[tokio::test]
async fn captures_per_endpoint_are_bounded_but_every_query_is_answered() {
    let harness = start_with(|settings| {
        lenient_sources(settings);
        settings.rate_capture_endpoint = rate("3/m:3");
    })
    .await;
    let packet = query(&harness.name(), RecordType::A);

    for _ in 0..10 {
        let reply = udp_exchange(harness.udp, &packet)
            .await
            .expect("the server answers");
        let message = decode(&reply);
        assert_eq!(message.metadata.response_code, ResponseCode::NoError);
        assert_eq!(message.answers.len(), 1);
    }

    assert_eq!(harness.rows().await.len(), 3);
    harness.stop().await;
}

#[tokio::test]
async fn a_store_failure_answers_servfail() {
    let harness = start().await;
    sqlx::query("ALTER TABLE capture_bodies RENAME TO capture_bodies_gone")
        .execute(harness.db.pool())
        .await
        .expect("the table is renamed");

    let reply = udp_exchange(harness.udp, &query(&harness.name(), RecordType::A))
        .await
        .expect("the server answers");

    assert_eq!(
        decode(&reply).metadata.response_code,
        ResponseCode::ServFail
    );
    harness.stop().await;
}

#[tokio::test]
async fn shutdown_stops_every_task() {
    let harness = start().await;

    tokio::time::timeout(WAIT * 10, harness.stop())
        .await
        .expect("the tasks end after the stop signal");
}

#[tokio::test]
async fn a_stale_enabled_state_is_served_when_the_lookup_budget_is_spent() {
    let harness = start_with(|settings| {
        lenient_sources(settings);
        settings.limits.cache_ttl = Duration::from_millis(100);
        settings.limits.lookup_rate = rate("1/m:1");
    })
    .await;
    let packet = query(&harness.name(), RecordType::A);
    udp_exchange(harness.udp, &packet)
        .await
        .expect("the server answers");
    tokio::time::sleep(Duration::from_millis(300)).await;

    let reply = udp_exchange(harness.udp, &packet)
        .await
        .expect("the stale state still answers");

    assert_eq!(decode(&reply).metadata.response_code, ResponseCode::NoError);
    assert_eq!(harness.handle.responder().database_lookups(), 1);
    assert_eq!(harness.rows().await.len(), 2);
    harness.stop().await;
}

#[tokio::test]
async fn a_full_storage_answers_without_storing() {
    let harness = start().await;
    harness
        .storage_full
        .store(true, std::sync::atomic::Ordering::Relaxed);

    let reply = udp_exchange(harness.udp, &query(&harness.name(), RecordType::A))
        .await
        .expect("the server answers");

    let message = decode(&reply);
    assert_eq!(message.metadata.response_code, ResponseCode::NoError);
    assert_eq!(message.answers.len(), 1);
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}
