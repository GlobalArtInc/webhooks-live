use std::net::IpAddr;

use whl_core::creator_net::{CreatorNet, CreatorNetError};

fn ip(text: &str) -> IpAddr {
    text.parse().expect("valid address")
}

#[test]
fn an_ipv4_address_is_reduced_to_its_slash_24() {
    let net = CreatorNet::from_ip(ip("203.0.113.77"));
    assert_eq!(net.to_string(), "203.0.113.0/24");
    assert!(net.contains(ip("203.0.113.1")));
    assert!(!net.contains(ip("203.0.114.1")));
}

#[test]
fn an_ipv6_address_is_reduced_to_its_slash_56() {
    let net = CreatorNet::from_ip(ip("2001:db8:aa:bb11:1:2:3:4"));
    assert_eq!(net.to_string(), "2001:db8:aa:bb00::/56");
    assert!(net.contains(ip("2001:db8:aa:bbff::9")));
    assert!(!net.contains(ip("2001:db8:aa:bc00::1")));
}

#[test]
fn an_ipv4_mapped_address_counts_as_the_ipv4_network() {
    let net = CreatorNet::from_ip(ip("::ffff:203.0.113.9"));
    assert_eq!(net, CreatorNet::from_ip(ip("203.0.113.200")));
    assert!(net.contains(ip("::ffff:203.0.113.10")));
}

#[test]
fn a_stored_network_parses_only_with_the_creator_prefix() {
    assert!("203.0.113.0/24".parse::<CreatorNet>().is_ok());
    assert!("2001:db8:aa:bb00::/56".parse::<CreatorNet>().is_ok());
    assert_eq!(
        "203.0.113.0/25".parse::<CreatorNet>(),
        Err(CreatorNetError::WrongPrefix)
    );
    assert_eq!(
        "203.0.113.5/24".parse::<CreatorNet>(),
        Err(CreatorNetError::WrongPrefix)
    );
    assert_eq!(
        "garbage".parse::<CreatorNet>(),
        Err(CreatorNetError::Unparsable)
    );
}
