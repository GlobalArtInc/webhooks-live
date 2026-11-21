use std::net::IpAddr;
use std::time::{Duration, Instant};

use hickory_proto::op::{Edns, Message, MessageType, OpCode, Query};
use hickory_proto::rr::{Name, RecordType};
use whl_core::endpoint_id::EndpointId;
use whl_dns::{
    EDNS_UDP_LIMIT, EndpointCache, NetworkKey, PLAIN_UDP_LIMIT, Parsed, Standing, Target, Zone,
    encode_within, parse, udp_limit,
};

fn name(text: &str) -> Name {
    let mut name = Name::from_ascii(text).expect("valid test name");
    name.set_fqdn(true);
    name
}

fn zone() -> Zone {
    Zone::new(
        name("dns.test.example"),
        name("ns.test.example"),
        name("hostmaster.test.example"),
        1,
        None,
        None,
    )
}

fn request_for(text: &str, edns: Option<Edns>) -> whl_dns::Request {
    let mut message = Message::new(7, MessageType::Query, OpCode::Query);
    message.add_query(Query::query(name(text), RecordType::SOA));
    if let Some(edns) = edns {
        message.set_edns(edns);
    }
    match parse(&message.to_vec().expect("encodes")) {
        Parsed::Query(request) => request,
        other => panic!("the query parses, got {other:?}"),
    }
}

#[test]
fn sources_in_one_v4_slash_24_share_a_key_and_other_networks_do_not() {
    let key = |text: &str| NetworkKey::from_ip(text.parse::<IpAddr>().expect("literal"));

    assert_eq!(key("203.0.113.1"), key("203.0.113.250"));
    assert_ne!(key("203.0.113.1"), key("203.0.114.1"));
}

#[test]
fn sources_in_one_v6_slash_56_share_a_key_and_other_networks_do_not() {
    let key = |text: &str| NetworkKey::from_ip(text.parse::<IpAddr>().expect("literal"));

    assert_eq!(key("2001:db8:0:100::1"), key("2001:db8:0:1ff:ffff::2"));
    assert_ne!(key("2001:db8:0:100::1"), key("2001:db8:0:200::1"));
}

#[test]
fn an_ipv4_mapped_source_is_keyed_like_the_plain_address() {
    let mapped: IpAddr = "::ffff:203.0.113.5".parse().expect("literal");
    let plain: IpAddr = "203.0.113.5".parse().expect("literal");

    assert_eq!(NetworkKey::from_ip(mapped), NetworkKey::from_ip(plain));
}

#[test]
fn a_cache_entry_expires_and_a_full_cache_refuses_new_ids() {
    let cache = EndpointCache::new(Duration::from_secs(10), Duration::from_secs(300), 1);
    let first = EndpointId::generate().expect("entropy");
    let second = EndpointId::generate().expect("entropy");
    let now = Instant::now();

    assert!(cache.put(first.clone(), Standing::Absent, now));
    assert!(!cache.put(second, Standing::Absent, now));
    assert_eq!(cache.get(&first, now), Some(Standing::Absent));
    assert_eq!(cache.get(&first, now + Duration::from_secs(11)), None);
    cache.sweep(now + Duration::from_secs(11));
    assert!(cache.is_empty());
}

#[test]
fn classification_matches_case_insensitively_and_splits_the_labels() {
    let id = EndpointId::generate().expect("entropy");
    let upper = id.unpack().to_uppercase();

    let target = zone().classify(&name(&format!("a.B.{upper}.DNS.test.EXAMPLE")));

    assert_eq!(
        target,
        Target::Captured {
            id,
            below: "a.B".to_owned()
        }
    );
}

#[test]
fn classification_separates_apex_outside_and_unknown_names() {
    let zone = zone();

    assert_eq!(zone.classify(&name("dns.test.example")), Target::Apex);
    assert_eq!(zone.classify(&name("test.example")), Target::Outside);
    assert_eq!(
        zone.classify(&name("x.dns.test.example.org")),
        Target::Outside
    );
    assert_eq!(
        zone.classify(&name("short.dns.test.example")),
        Target::Unknown
    );
    assert_eq!(zone.classify(&name("*.dns.test.example")), Target::Unknown);
}

#[test]
fn a_response_above_the_limit_is_replaced_by_a_truncated_one_without_records() {
    let zone = zone();
    let request = request_for("dns.test.example", None);
    let answer = zone.apex_answer(&request);
    let full = encode_within(&answer, &request, None).expect("encodes");

    let cut = encode_within(&answer, &request, Some(full.len() - 1)).expect("encodes");

    let message = Message::from_vec(&cut).expect("decodes");
    assert!(message.metadata.truncation);
    assert!(message.answers.is_empty());
    assert!(cut.len() < full.len());
}

#[test]
fn the_udp_limit_follows_the_advertised_size_between_512_and_1232() {
    let limit = |size: Option<u16>| {
        let edns = size.map(|size| {
            let mut edns = Edns::new();
            edns.set_max_payload(size);
            edns
        });
        udp_limit(&request_for("dns.test.example", edns))
    };

    assert_eq!(limit(None), PLAIN_UDP_LIMIT);
    assert_eq!(limit(Some(700)), 700);
    assert_eq!(limit(Some(4096)), EDNS_UDP_LIMIT);
    assert_eq!(limit(Some(100)), PLAIN_UDP_LIMIT);
}

#[test]
fn an_enabled_entry_stays_usable_as_stale_after_it_expires_but_an_absent_one_does_not() {
    let cache = EndpointCache::new(Duration::from_secs(10), Duration::from_secs(300), 10);
    let enabled = EndpointId::generate().expect("entropy");
    let absent = EndpointId::generate().expect("entropy");
    let now = Instant::now();
    cache.put(enabled.clone(), Standing::Enabled, now);
    cache.put(absent.clone(), Standing::Absent, now);
    let later = now + Duration::from_secs(60);

    assert_eq!(cache.get(&enabled, later), None);
    assert!(cache.get_stale_enabled(&enabled, later));
    assert!(!cache.get_stale_enabled(&absent, later));
    assert!(!cache.get_stale_enabled(&enabled, now + Duration::from_secs(311)));
    cache.sweep(later);
    assert_eq!(cache.len(), 1);
}

#[test]
fn a_connection_table_caps_one_address_and_forgets_it_when_the_guards_drop() {
    let table = whl_dns::ConnectionTable::new(2);
    let address: IpAddr = "203.0.113.1".parse().expect("literal");
    let other: IpAddr = "203.0.113.2".parse().expect("literal");

    let first = table.enter(address).expect("first fits");
    let second = table.enter(address).expect("second fits");
    assert!(table.enter(address).is_none());
    assert!(table.enter(other).is_some());
    drop((first, second));

    assert_eq!(table.tracked_addresses(), 0);
    assert!(table.enter(address).is_some());
}
