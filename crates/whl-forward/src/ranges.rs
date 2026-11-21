use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

#[derive(Debug, Clone, Copy)]
pub struct RefusedIpv4Range {
    pub net: Ipv4Net,
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Embedding {
    LastThirtyTwoBits,
    SixToFour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Treatment {
    Refuse,
    EmbeddedIpv4(Embedding),
}

#[derive(Debug, Clone, Copy)]
pub struct RefusedIpv6Range {
    pub net: Ipv6Net,
    pub name: &'static str,
    pub treatment: Treatment,
}

const fn v4(octets: [u8; 4], prefix_len: u8, name: &'static str) -> RefusedIpv4Range {
    RefusedIpv4Range {
        net: Ipv4Net::new_assert(
            Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]),
            prefix_len,
        ),
        name,
    }
}

const fn v6(
    segments: [u16; 8],
    prefix_len: u8,
    name: &'static str,
    treatment: Treatment,
) -> RefusedIpv6Range {
    RefusedIpv6Range {
        net: Ipv6Net::new_assert(
            Ipv6Addr::new(
                segments[0],
                segments[1],
                segments[2],
                segments[3],
                segments[4],
                segments[5],
                segments[6],
                segments[7],
            ),
            prefix_len,
        ),
        name,
        treatment,
    }
}

pub const REFUSED_IPV4_RANGES: [RefusedIpv4Range; 17] = [
    v4([0, 0, 0, 0], 8, "this-network"),
    v4([10, 0, 0, 0], 8, "private-use-10"),
    v4([100, 64, 0, 0], 10, "shared-address-space"),
    v4([127, 0, 0, 0], 8, "loopback"),
    v4([169, 254, 0, 0], 16, "link-local-and-metadata"),
    v4([172, 16, 0, 0], 12, "private-use-172"),
    v4([192, 0, 0, 0], 24, "ietf-protocol-assignments"),
    v4([192, 0, 2, 0], 24, "documentation-test-net-1"),
    v4([192, 88, 99, 0], 24, "six-to-four-relay-anycast"),
    v4([192, 168, 0, 0], 16, "private-use-192"),
    v4([198, 18, 0, 0], 15, "benchmarking"),
    v4([198, 51, 100, 0], 24, "documentation-test-net-2"),
    v4([203, 0, 113, 0], 24, "documentation-test-net-3"),
    v4([224, 0, 0, 0], 4, "multicast"),
    v4([240, 0, 0, 0], 4, "reserved"),
    v4([255, 255, 255, 255], 32, "limited-broadcast"),
    v4([168, 63, 129, 16], 32, "azure-wireserver"),
];

pub const REFUSED_IPV6_RANGES: [RefusedIpv6Range; 14] = [
    v6(
        [0, 0, 0, 0, 0, 0, 0, 0],
        128,
        "unspecified",
        Treatment::Refuse,
    ),
    v6([0, 0, 0, 0, 0, 0, 0, 1], 128, "loopback", Treatment::Refuse),
    v6(
        [0, 0, 0, 0, 0, 0, 0, 0],
        96,
        "ipv4-compatible",
        Treatment::Refuse,
    ),
    v6(
        [0, 0, 0, 0, 0, 0xffff, 0, 0],
        96,
        "ipv4-mapped",
        Treatment::EmbeddedIpv4(Embedding::LastThirtyTwoBits),
    ),
    v6(
        [0x64, 0xff9b, 0, 0, 0, 0, 0, 0],
        96,
        "nat64",
        Treatment::EmbeddedIpv4(Embedding::LastThirtyTwoBits),
    ),
    v6(
        [0x64, 0xff9b, 1, 0, 0, 0, 0, 0],
        48,
        "nat64-local-use",
        Treatment::Refuse,
    ),
    v6(
        [0x100, 0, 0, 0, 0, 0, 0, 0],
        64,
        "discard-only",
        Treatment::Refuse,
    ),
    v6(
        [0x2001, 0, 0, 0, 0, 0, 0, 0],
        23,
        "ietf-protocol-assignments",
        Treatment::Refuse,
    ),
    v6(
        [0x2001, 0xdb8, 0, 0, 0, 0, 0, 0],
        32,
        "documentation",
        Treatment::Refuse,
    ),
    v6(
        [0x2002, 0, 0, 0, 0, 0, 0, 0],
        16,
        "six-to-four",
        Treatment::EmbeddedIpv4(Embedding::SixToFour),
    ),
    v6(
        [0x3fff, 0, 0, 0, 0, 0, 0, 0],
        20,
        "documentation-large",
        Treatment::Refuse,
    ),
    v6(
        [0xfc00, 0, 0, 0, 0, 0, 0, 0],
        7,
        "unique-local",
        Treatment::Refuse,
    ),
    v6(
        [0xfe80, 0, 0, 0, 0, 0, 0, 0],
        10,
        "link-local",
        Treatment::Refuse,
    ),
    v6(
        [0xff00, 0, 0, 0, 0, 0, 0, 0],
        8,
        "multicast",
        Treatment::Refuse,
    ),
];

pub const GLOBAL_UNICAST_IPV6: Ipv6Net =
    Ipv6Net::new_assert(Ipv6Addr::new(0x2000, 0, 0, 0, 0, 0, 0, 0), 3);

pub const OUTSIDE_GLOBAL_UNICAST_NAME: &str = "outside-global-unicast";

fn embedded_ipv4(address: Ipv6Addr, embedding: Embedding) -> Ipv4Addr {
    let octets = address.octets();
    match embedding {
        Embedding::LastThirtyTwoBits => {
            Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15])
        }
        Embedding::SixToFour => Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]),
    }
}

fn refusing_ipv6_range(address: Ipv6Addr, allow: &[IpNet]) -> Option<&'static str> {
    let mut matching = REFUSED_IPV6_RANGES
        .iter()
        .filter(|range| range.net.contains(&address))
        .peekable();
    if matching.peek().is_none() {
        return if GLOBAL_UNICAST_IPV6.contains(&address) {
            None
        } else {
            Some(OUTSIDE_GLOBAL_UNICAST_NAME)
        };
    }
    matching.find_map(|range| match range.treatment {
        Treatment::Refuse => Some(range.name),
        Treatment::EmbeddedIpv4(embedding) => {
            refusing_range(IpAddr::V4(embedded_ipv4(address, embedding)), allow)
        }
    })
}

pub fn refusing_range(address: IpAddr, allow: &[IpNet]) -> Option<&'static str> {
    if allow.iter().any(|net| net.contains(&address)) {
        return None;
    }
    match address {
        IpAddr::V4(v4) => REFUSED_IPV4_RANGES
            .iter()
            .find(|range| range.net.contains(&v4))
            .map(|range| range.name),
        IpAddr::V6(v6) => refusing_ipv6_range(v6, allow),
    }
}

pub fn is_refused(address: IpAddr, allow: &[IpNet]) -> bool {
    refusing_range(address, allow).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn net(text: &str) -> IpNet {
        text.parse().unwrap()
    }

    fn mapped(v4: Ipv4Addr) -> Ipv6Addr {
        let [a, b, c, d] = v4.octets();
        Ipv6Addr::new(
            0,
            0,
            0,
            0,
            0,
            0xffff,
            u16::from_be_bytes([a, b]),
            u16::from_be_bytes([c, d]),
        )
    }

    fn nat64(v4: Ipv4Addr) -> Ipv6Addr {
        let [a, b, c, d] = v4.octets();
        Ipv6Addr::new(
            0x64,
            0xff9b,
            0,
            0,
            0,
            0,
            u16::from_be_bytes([a, b]),
            u16::from_be_bytes([c, d]),
        )
    }

    fn six_to_four(v4: Ipv4Addr) -> Ipv6Addr {
        let [a, b, c, d] = v4.octets();
        Ipv6Addr::new(
            0x2002,
            u16::from_be_bytes([a, b]),
            u16::from_be_bytes([c, d]),
            0,
            0,
            0,
            0,
            1,
        )
    }

    fn compatible(v4: Ipv4Addr) -> Ipv6Addr {
        let [a, b, c, d] = v4.octets();
        Ipv6Addr::new(
            0,
            0,
            0,
            0,
            0,
            0,
            u16::from_be_bytes([a, b]),
            u16::from_be_bytes([c, d]),
        )
    }

    #[test]
    fn refuses_representatives_and_boundaries_of_every_range() {
        let refused = [
            ("0.0.0.0", "this-network"),
            ("0.255.255.255", "this-network"),
            ("10.0.0.0", "private-use-10"),
            ("10.255.255.255", "private-use-10"),
            ("100.64.0.0", "shared-address-space"),
            ("100.127.255.255", "shared-address-space"),
            ("100.100.100.200", "shared-address-space"),
            ("127.0.0.1", "loopback"),
            ("127.255.255.255", "loopback"),
            ("169.254.0.0", "link-local-and-metadata"),
            ("169.254.169.254", "link-local-and-metadata"),
            ("169.254.255.255", "link-local-and-metadata"),
            ("172.16.0.0", "private-use-172"),
            ("172.31.255.255", "private-use-172"),
            ("192.0.0.1", "ietf-protocol-assignments"),
            ("192.0.2.1", "documentation-test-net-1"),
            ("192.88.99.1", "six-to-four-relay-anycast"),
            ("192.168.0.0", "private-use-192"),
            ("192.168.255.255", "private-use-192"),
            ("198.18.0.0", "benchmarking"),
            ("198.19.255.255", "benchmarking"),
            ("198.51.100.7", "documentation-test-net-2"),
            ("203.0.113.7", "documentation-test-net-3"),
            ("224.0.0.1", "multicast"),
            ("239.255.255.255", "multicast"),
            ("240.0.0.1", "reserved"),
            ("255.255.255.255", "reserved"),
            ("168.63.129.16", "azure-wireserver"),
            ("::", "unspecified"),
            ("::1", "loopback"),
            ("::2", "ipv4-compatible"),
            ("100::1", "discard-only"),
            ("100::ffff:ffff:ffff:ffff", "discard-only"),
            ("2001::1", "ietf-protocol-assignments"),
            ("2001:1ff:ffff::1", "ietf-protocol-assignments"),
            ("2001:db8::1", "documentation"),
            ("3fff::1", "documentation-large"),
            ("3fff:fff:ffff::1", "documentation-large"),
            ("fc00::1", "unique-local"),
            ("fdff:ffff::1", "unique-local"),
            ("fe80::1", "link-local"),
            ("febf::1", "link-local"),
            ("ff02::1", "multicast"),
            ("64:ff9b:1::1", "nat64-local-use"),
            ("::ffff:0:0:1", "outside-global-unicast"),
            ("::ffff:0:1", "this-network"),
            ("4000::1", "outside-global-unicast"),
            ("5f00::1", "outside-global-unicast"),
            ("fec0::1", "outside-global-unicast"),
        ];
        for (text, name) in refused {
            assert_eq!(refusing_range(ip(text), &[]), Some(name), "{text}");
        }
    }

    #[test]
    fn accepts_addresses_just_outside_every_range() {
        let accepted = [
            "1.0.0.0",
            "9.255.255.255",
            "11.0.0.0",
            "100.63.255.255",
            "100.128.0.0",
            "126.255.255.255",
            "128.0.0.0",
            "169.253.255.255",
            "169.255.0.0",
            "172.15.255.255",
            "172.32.0.0",
            "192.0.1.1",
            "192.0.3.1",
            "192.88.98.255",
            "192.88.100.0",
            "192.167.255.255",
            "192.169.0.0",
            "198.17.255.255",
            "198.20.0.0",
            "198.51.99.255",
            "198.51.101.0",
            "203.0.112.255",
            "203.0.114.0",
            "223.255.255.255",
            "168.63.129.15",
            "168.63.129.17",
            "2001:200::1",
            "2001:db9::1",
            "3fff:1000::1",
            "2000::1",
            "8.8.8.8",
            "1.1.1.1",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
        ];
        for text in accepted {
            assert_eq!(refusing_range(ip(text), &[]), None, "{text}");
        }
    }

    #[test]
    fn every_table_entry_refuses_its_first_and_last_address() {
        for range in REFUSED_IPV4_RANGES {
            assert!(
                is_refused(IpAddr::V4(range.net.network()), &[]),
                "{}",
                range.name
            );
            assert!(
                is_refused(IpAddr::V4(range.net.broadcast()), &[]),
                "{}",
                range.name
            );
        }
        for range in REFUSED_IPV6_RANGES {
            assert!(
                is_refused(IpAddr::V6(range.net.network()), &[]),
                "{}",
                range.name
            );
            assert!(
                is_refused(IpAddr::V6(range.net.broadcast()), &[]),
                "{}",
                range.name
            );
        }
    }

    #[test]
    fn applies_ipv4_rules_to_mapped_nat64_six_to_four_and_compatible_forms() {
        let private = [
            Ipv4Addr::new(10, 1, 2, 3),
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 169, 254),
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(172, 16, 0, 1),
            Ipv4Addr::new(100, 64, 0, 1),
        ];
        for v4 in private {
            for form in [mapped(v4), nat64(v4), six_to_four(v4), compatible(v4)] {
                assert!(is_refused(IpAddr::V6(form), &[]), "{form}");
            }
        }
    }

    #[test]
    fn accepts_mapped_nat64_and_six_to_four_forms_of_public_ipv4() {
        let public = Ipv4Addr::new(8, 8, 8, 8);
        for form in [mapped(public), nat64(public), six_to_four(public)] {
            assert!(!is_refused(IpAddr::V6(form), &[]), "{form}");
        }
        assert!(is_refused(IpAddr::V6(compatible(public)), &[]));
    }

    #[test]
    fn allow_list_wins_over_refusal() {
        let allow = [net("10.0.0.0/8"), net("127.0.0.1/32"), net("fd00::/8")];
        assert!(!is_refused(ip("10.1.2.3"), &allow));
        assert!(!is_refused(ip("127.0.0.1"), &allow));
        assert!(!is_refused(ip("fd00::1"), &allow));
        assert!(!is_refused(ip("::ffff:10.1.2.3"), &allow));
        assert!(!is_refused(ip("64:ff9b::a01:203"), &allow));
        assert!(is_refused(ip("127.0.0.2"), &allow));
        assert!(is_refused(ip("192.168.1.1"), &allow));
        assert!(is_refused(ip("169.254.169.254"), &allow));
        assert!(is_refused(ip("fc00::1"), &allow));
        assert!(is_refused(ip("::1"), &allow));
        assert!(is_refused(ip("::ffff:127.0.0.2"), &allow));
    }
}
