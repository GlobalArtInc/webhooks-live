use std::net::IpAddr;
use std::str::FromStr;

use axum::http::HeaderMap;
use ipnet::IpNet;

const FORWARDED_FOR: &str = "x-forwarded-for";
const FORWARDED_PROTO: &str = "x-forwarded-proto";
const FORWARDED_HOST: &str = "x-forwarded-host";
const MAX_HOST_BYTES: usize = 255;
const HOST_REPLACEMENT: char = '_';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedRequest {
    pub client_ip: IpAddr,
    pub scheme: Scheme,
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct TrustedProxies {
    nets: Vec<IpNet>,
}

impl TrustedProxies {
    pub fn new(nets: Vec<IpNet>) -> Self {
        Self { nets }
    }

    pub fn contains(&self, address: IpAddr) -> bool {
        let address = address.to_canonical();
        self.nets.iter().any(|net| net.contains(&address))
    }

    pub fn resolve(&self, peer: IpAddr, headers: &HeaderMap) -> ForwardedRequest {
        let peer = peer.to_canonical();
        if !self.contains(peer) {
            return ForwardedRequest {
                client_ip: peer,
                scheme: Scheme::Http,
                host: None,
            };
        }
        ForwardedRequest {
            client_ip: self.walk_chain(peer, headers),
            scheme: forwarded_scheme(headers),
            host: forwarded_host(headers),
        }
    }

    fn walk_chain(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        let chain = joined_values(headers, FORWARDED_FOR);
        let mut nearest = peer;
        for entry in chain.rsplit(',') {
            let Ok(address) = IpAddr::from_str(entry.trim()) else {
                return nearest;
            };
            let address = address.to_canonical();
            if !self.contains(address) {
                return address;
            }
            nearest = address;
        }
        nearest
    }
}

fn joined_values(headers: &HeaderMap, name: &str) -> String {
    headers
        .get_all(name)
        .iter()
        .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
        .collect::<Vec<_>>()
        .join(",")
}

fn rightmost_entry(headers: &HeaderMap, name: &str) -> Option<String> {
    joined_values(headers, name)
        .rsplit(',')
        .map(str::trim)
        .find(|entry| !entry.is_empty())
        .map(str::to_owned)
}

fn forwarded_scheme(headers: &HeaderMap) -> Scheme {
    match rightmost_entry(headers, FORWARDED_PROTO)
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("https") => Scheme::Https,
        _ => Scheme::Http,
    }
}

fn forwarded_host(headers: &HeaderMap) -> Option<String> {
    rightmost_entry(headers, FORWARDED_HOST).map(|value| sanitize_host(&value))
}

pub fn sanitize_host(raw: &str) -> String {
    let mut host: String = raw
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || "-._:[]".contains(character) {
                character
            } else {
                HOST_REPLACEMENT
            }
        })
        .collect();
    host.truncate(MAX_HOST_BYTES);
    host
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proxies(nets: &[&str]) -> TrustedProxies {
        TrustedProxies::new(
            nets.iter()
                .map(|net| net.parse().expect("valid CIDR"))
                .collect(),
        )
    }

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).expect("valid name"),
                value.parse().expect("valid value"),
            );
        }
        map
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("valid address")
    }

    #[test]
    fn untrusted_peer_ignores_every_forwarded_header() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[
            ("x-forwarded-for", "1.1.1.1"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "evil.example"),
        ]);
        let resolved = trusted.resolve(ip("203.0.113.5"), &map);
        assert_eq!(resolved.client_ip, ip("203.0.113.5"));
        assert_eq!(resolved.scheme, Scheme::Http);
        assert_eq!(resolved.host, None);
    }

    #[test]
    fn trusted_peer_without_forwarded_for_is_the_client() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let resolved = trusted.resolve(ip("10.1.2.3"), &HeaderMap::new());
        assert_eq!(resolved.client_ip, ip("10.1.2.3"));
    }

    #[test]
    fn trusted_peer_walks_the_chain_right_to_left() {
        let trusted = proxies(&["10.0.0.0/8", "192.168.0.0/16"]);
        let map = headers(&[("x-forwarded-for", "198.51.100.7, 10.9.9.9, 192.168.1.1")]);
        let resolved = trusted.resolve(ip("10.0.0.2"), &map);
        assert_eq!(resolved.client_ip, ip("198.51.100.7"));
    }

    #[test]
    fn spoofed_leftmost_value_is_ignored() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[("x-forwarded-for", "6.6.6.6, 203.0.113.9")]);
        let resolved = trusted.resolve(ip("10.0.0.2"), &map);
        assert_eq!(resolved.client_ip, ip("203.0.113.9"));
    }

    #[test]
    fn repeated_forwarded_for_lines_form_one_chain() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[
            ("x-forwarded-for", "6.6.6.6"),
            ("x-forwarded-for", "203.0.113.9, 10.4.4.4"),
        ]);
        let resolved = trusted.resolve(ip("10.0.0.2"), &map);
        assert_eq!(resolved.client_ip, ip("203.0.113.9"));
    }

    #[test]
    fn garbage_entry_falls_back_to_the_nearest_trusted_hop() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[("x-forwarded-for", "1.2.3.4, not-an-ip, 10.5.5.5")]);
        let resolved = trusted.resolve(ip("10.0.0.2"), &map);
        assert_eq!(resolved.client_ip, ip("10.5.5.5"));
    }

    #[test]
    fn all_trusted_chain_yields_the_leftmost_entry() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[("x-forwarded-for", "10.7.7.7, 10.8.8.8")]);
        let resolved = trusted.resolve(ip("10.0.0.2"), &map);
        assert_eq!(resolved.client_ip, ip("10.7.7.7"));
    }

    #[test]
    fn ipv4_mapped_peer_matches_an_ipv4_proxy_range() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[("x-forwarded-for", "203.0.113.9")]);
        let resolved = trusted.resolve(ip("::ffff:10.0.0.2"), &map);
        assert_eq!(resolved.client_ip, ip("203.0.113.9"));
    }

    #[test]
    fn trusted_peer_supplies_scheme_and_host_from_the_rightmost_entry() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[
            ("x-forwarded-proto", "http, https"),
            ("x-forwarded-host", "spoof.example, hooks.example.com"),
        ]);
        let resolved = trusted.resolve(ip("10.0.0.2"), &map);
        assert_eq!(resolved.scheme, Scheme::Https);
        assert_eq!(resolved.host.as_deref(), Some("hooks.example.com"));
    }

    #[test]
    fn unknown_forwarded_proto_falls_back_to_http() {
        let trusted = proxies(&["10.0.0.0/8"]);
        let map = headers(&[("x-forwarded-proto", "gopher")]);
        let resolved = trusted.resolve(ip("10.0.0.2"), &map);
        assert_eq!(resolved.scheme, Scheme::Http);
    }

    #[test]
    fn host_is_restricted_to_a_safe_alphabet() {
        assert_eq!(sanitize_host("a.b/c@d e"), "a.b_c_d_e");
        assert_eq!(sanitize_host(&"a".repeat(400)).len(), MAX_HOST_BYTES);
    }
}
