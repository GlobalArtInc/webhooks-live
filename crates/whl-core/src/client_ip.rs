use std::net::IpAddr;
use std::str::FromStr;

use http::HeaderMap;
use ipnet::IpNet;

const FORWARDED_FOR: &str = "x-forwarded-for";
const FORWARDED_PROTO: &str = "x-forwarded-proto";
const FORWARDED_HOST: &str = "x-forwarded-host";
pub const MAX_HOST_BYTES: usize = 255;
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
