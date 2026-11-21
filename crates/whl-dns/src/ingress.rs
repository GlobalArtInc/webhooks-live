use std::net::{IpAddr, SocketAddr};

use whl_core::client_ip::TrustedProxies;
use whl_core::proxy_protocol::{self, ProxyError, ProxyHeader};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IngressError {
    #[error("the PROXY header from a trusted proxy is invalid: {0}")]
    Proxy(#[from] ProxyError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ingress<'a> {
    pub resolver: IpAddr,
    pub query: &'a [u8],
}

pub fn resolver_of(header: ProxyHeader, peer: IpAddr) -> IpAddr {
    match header {
        ProxyHeader::Local => peer.to_canonical(),
        ProxyHeader::Proxied(addresses) => addresses.source.ip().to_canonical(),
    }
}

pub fn datagram_ingress<'a>(
    trusted: &TrustedProxies,
    peer: SocketAddr,
    datagram: &'a [u8],
) -> Result<Ingress<'a>, IngressError> {
    if !trusted.contains(peer.ip()) {
        return Ok(Ingress {
            resolver: peer.ip().to_canonical(),
            query: datagram,
        });
    }
    let parsed = proxy_protocol::parse(datagram)?;
    Ok(Ingress {
        resolver: resolver_of(parsed.header, peer.ip()),
        query: &datagram[parsed.consumed..],
    })
}
