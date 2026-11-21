mod common;

use common::{
    ZONE, decode, query, query_in_class, query_message, query_with_edns, query_with_subnet, start,
    start_with, tcp_exchange, udp_exchange,
};
use hickory_proto::op::{OpCode, Query, ResponseCode};
use hickory_proto::rr::{DNSClass, Name, RData, RecordType};

#[tokio::test]
async fn a_query_over_udp_is_answered_authoritatively_and_stored() {
    let harness = start().await;
    let name = harness.name();

    let reply = udp_exchange(harness.udp, &query(&name, RecordType::A))
        .await
        .expect("the server answers");
    let message = decode(&reply);

    assert_eq!(message.metadata.id, 0x1234);
    assert!(message.metadata.authoritative);
    assert!(!message.metadata.recursion_available);
    assert_eq!(message.metadata.response_code, ResponseCode::NoError);
    assert_eq!(message.answers.len(), 1);
    assert_eq!(message.answers[0].ttl, 0);
    assert_eq!(
        message.answers[0].data,
        RData::A("192.0.2.7".parse().expect("literal address"))
    );
    let rows = harness.rows().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "dns");
    assert_eq!(rows[0].method, "A");
    assert_eq!(rows[0].url, format!("dns:{name}."));
    assert_eq!(rows[0].host, ZONE);
    assert_eq!(rows[0].client_ip, "127.0.0.1/32");
    assert_eq!(rows[0].response_status, 0);
    assert_eq!(rows[0].meta["transport"], "udp");
    assert_eq!(rows[0].meta["type"], "A");
    assert_eq!(rows[0].meta["class"], "IN");
    assert_eq!(rows[0].meta["dns_id"], 0x1234);
    assert_eq!(rows[0].meta["flags"]["rd"], true);
    assert_eq!(rows[0].meta["edns_udp_size"], serde_json::Value::Null);
    harness.stop().await;
}

#[tokio::test]
async fn the_same_query_over_tcp_is_answered_and_stored_as_tcp() {
    let harness = start().await;
    let name = harness.name();

    let reply = tcp_exchange(harness.tcp, &query(&name, RecordType::AAAA))
        .await
        .expect("the server answers");

    let message = decode(&reply);
    assert_eq!(
        message.answers[0].data,
        RData::AAAA("2001:db8::7".parse().expect("literal address"))
    );
    let rows = harness.rows().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].meta["transport"], "tcp");
    assert_eq!(rows[0].method, "AAAA");
    harness.stop().await;
}

#[tokio::test]
async fn a_case_randomised_name_is_matched_and_stored_as_received() {
    let harness = start().await;
    let name = format!(
        "{}.Dns.TEST.example",
        harness.endpoint.unpack().to_uppercase()
    );

    let reply = udp_exchange(harness.udp, &query(&name, RecordType::A))
        .await
        .expect("the server answers");

    let message = decode(&reply);
    assert_eq!(message.metadata.response_code, ResponseCode::NoError);
    assert_eq!(message.queries[0].name().to_ascii(), format!("{name}."));
    assert_eq!(message.answers[0].name.to_ascii(), format!("{name}."));
    let rows = harness.rows().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].meta["name"], format!("{name}."));
    harness.stop().await;
}

#[tokio::test]
async fn a_name_below_the_id_is_captured_with_its_labels_in_the_path() {
    let harness = start().await;
    let name = format!("token.deep.{}", harness.name());

    udp_exchange(harness.udp, &query(&name, RecordType::A))
        .await
        .expect("the server answers");

    let rows = harness.rows().await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "token.deep");
    harness.stop().await;
}

#[tokio::test]
async fn txt_answers_with_the_capture_id() {
    let harness = start().await;

    let reply = udp_exchange(harness.udp, &query(&harness.name(), RecordType::TXT))
        .await
        .expect("the server answers");

    let message = decode(&reply);
    let rows = harness.rows().await;
    let RData::TXT(text) = &message.answers[0].data else {
        panic!("the answer is a TXT record");
    };
    assert_eq!(text.to_string(), rows[0].id);
    harness.stop().await;
}

#[tokio::test]
async fn any_gets_the_minimal_hinfo_answer() {
    let harness = start().await;

    let reply = udp_exchange(harness.udp, &query(&harness.name(), RecordType::ANY))
        .await
        .expect("the server answers");

    let message = decode(&reply);
    assert_eq!(message.answers.len(), 1);
    assert!(matches!(message.answers[0].data, RData::HINFO(_)));
    assert_eq!(harness.rows().await[0].method, "ANY");
    harness.stop().await;
}

#[tokio::test]
async fn other_types_get_no_data_without_a_configured_address() {
    let harness = start_with(|settings| settings.answer_a = None).await;
    let name = harness.name();

    let mx = decode(
        &udp_exchange(harness.udp, &query(&name, RecordType::MX))
            .await
            .expect("the server answers"),
    );
    let a = decode(
        &udp_exchange(harness.udp, &query(&name, RecordType::A))
            .await
            .expect("the server answers"),
    );

    for message in [mx, a] {
        assert_eq!(message.metadata.response_code, ResponseCode::NoError);
        assert!(message.answers.is_empty());
    }
    assert_eq!(harness.rows().await.len(), 2);
    harness.stop().await;
}

#[tokio::test]
async fn the_apex_answers_soa_and_ns_and_is_not_stored() {
    let harness = start().await;

    let soa = decode(
        &udp_exchange(harness.udp, &query(ZONE, RecordType::SOA))
            .await
            .expect("the server answers"),
    );
    let ns = decode(
        &udp_exchange(harness.udp, &query(ZONE, RecordType::NS))
            .await
            .expect("the server answers"),
    );

    let RData::SOA(record) = &soa.answers[0].data else {
        panic!("the answer is an SOA record");
    };
    assert_eq!(record.minimum, 60);
    assert_eq!(record.rname.to_ascii(), "hostmaster.test.example.");
    assert!(matches!(ns.answers[0].data, RData::NS(_)));
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn names_outside_the_zone_are_refused() {
    let harness = start().await;

    let reply = udp_exchange(harness.udp, &query("example.org", RecordType::A))
        .await
        .expect("the server answers");

    let message = decode(&reply);
    assert_eq!(message.metadata.response_code, ResponseCode::Refused);
    assert!(!message.metadata.authoritative);
    assert!(message.answers.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn a_class_other_than_internet_is_refused() {
    let harness = start().await;

    let reply = udp_exchange(harness.udp, &query_in_class(&harness.name(), DNSClass::CH))
        .await
        .expect("the server answers");

    assert_eq!(decode(&reply).metadata.response_code, ResponseCode::Refused);
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn an_opcode_other_than_query_gets_notimp() {
    let harness = start().await;
    let mut message = query_message(&harness.name(), RecordType::A);
    message.metadata.op_code = OpCode::Notify;

    let reply = udp_exchange(harness.udp, &message.to_vec().expect("encodes"))
        .await
        .expect("the server answers");

    assert_eq!(decode(&reply).metadata.response_code, ResponseCode::NotImp);
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn two_questions_get_formerr() {
    let harness = start().await;
    let mut message = query_message(&harness.name(), RecordType::A);
    message.add_query(Query::query(
        Name::from_ascii(harness.name()).expect("valid"),
        RecordType::TXT,
    ));

    let reply = udp_exchange(harness.udp, &message.to_vec().expect("encodes"))
        .await
        .expect("the server answers");

    assert_eq!(decode(&reply).metadata.response_code, ResponseCode::FormErr);
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn edns_is_echoed_and_the_client_subnet_and_size_are_stored() {
    let harness = start().await;

    let reply = udp_exchange(
        harness.udp,
        &query_with_subnet(&harness.name(), RecordType::A),
    )
    .await
    .expect("the server answers");

    let message = decode(&reply);
    assert!(message.edns.is_some());
    let rows = harness.rows().await;
    assert_eq!(rows[0].meta["client_subnet"], "198.51.100.0/24");
    assert_eq!(rows[0].meta["edns_udp_size"], 1232);
    harness.stop().await;
}

#[tokio::test]
async fn an_unsupported_edns_version_gets_badvers() {
    let harness = start().await;
    let mut message = query_message(&harness.name(), RecordType::A);
    let mut edns = hickory_proto::op::Edns::new();
    edns.set_version(1);
    message.set_edns(edns);

    let reply = udp_exchange(harness.udp, &message.to_vec().expect("encodes"))
        .await
        .expect("the server answers");

    assert_eq!(reply[3] & 0x0f, 0);
    assert_eq!(
        decode(&reply)
            .edns
            .expect("the reply carries OPT")
            .rcode_high(),
        1
    );
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn an_unknown_label_gets_nxdomain_with_the_soa_and_is_not_stored() {
    let harness = start().await;

    let reply = udp_exchange(
        harness.udp,
        &query(&format!("not-an-endpoint.{ZONE}"), RecordType::A),
    )
    .await
    .expect("the server answers");

    let message = decode(&reply);
    assert_eq!(message.metadata.response_code, ResponseCode::NXDomain);
    assert!(message.metadata.authoritative);
    assert!(matches!(message.authorities[0].data, RData::SOA(_)));
    assert!(harness.rows().await.is_empty());
    harness.stop().await;
}

#[tokio::test]
async fn every_answer_type_stays_small() {
    let harness = start().await;
    let name = harness.name();
    let long = format!("{}.{}", vec!["a".repeat(60); 3].join("."), ZONE);
    let queries = [
        query(&name, RecordType::A),
        query(&name, RecordType::AAAA),
        query(&name, RecordType::TXT),
        query(&name, RecordType::ANY),
        query(ZONE, RecordType::SOA),
        query(ZONE, RecordType::NS),
        query(ZONE, RecordType::ANY),
        query(&long, RecordType::A),
        query_with_edns(&name, RecordType::ANY, 4096),
    ];

    for packet in queries {
        let reply = udp_exchange(harness.udp, &packet)
            .await
            .expect("the server answers");
        assert!(reply.len() <= 512, "{} bytes", reply.len());
        assert!(
            reply.len() <= packet.len() * 4,
            "{} bytes for a {} byte query",
            reply.len(),
            packet.len()
        );
    }
    harness.stop().await;
}
