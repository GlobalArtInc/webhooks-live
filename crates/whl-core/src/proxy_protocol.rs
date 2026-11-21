use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const SIGNATURE: [u8; 12] = [
    0x0D, 0x0A, 0x0D, 0x0A, 0x00, 0x0D, 0x0A, 0x51, 0x55, 0x49, 0x54, 0x0A,
];
pub const PREFIX_LEN: usize = 16;
pub const MAX_PAYLOAD_LEN: usize = 512;

const VERSION: u8 = 2;
const COMMAND_LOCAL: u8 = 0;
const COMMAND_PROXY: u8 = 1;
const FAMILY_UNSPEC: u8 = 0;
const FAMILY_INET: u8 = 1;
const FAMILY_INET6: u8 = 2;
const TRANSPORT_STREAM: u8 = 1;
const INET_ADDRESS_LEN: usize = 12;
const INET6_ADDRESS_LEN: usize = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProxyError {
    #[error("the PROXY protocol signature is missing")]
    BadSignature,
    #[error("PROXY protocol version {0} is not supported")]
    UnsupportedVersion(u8),
    #[error("PROXY protocol command {0} is not supported")]
    UnsupportedCommand(u8),
    #[error("the PROXY protocol address family or transport is not supported")]
    UnsupportedTransport,
    #[error("the PROXY protocol payload of {0} bytes exceeds the limit of {MAX_PAYLOAD_LEN}")]
    PayloadTooLarge(usize),
    #[error(
        "the PROXY protocol payload of {got} bytes is shorter than the {needed} the address block needs"
    )]
    PayloadTooShort { needed: usize, got: usize },
    #[error("the PROXY protocol header is cut short")]
    Truncated,
    #[error("the PROXY protocol source address is not usable")]
    InvalidAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyHeader {
    Local,
    Proxied {
        source: SocketAddr,
        destination: SocketAddr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Local,
    Inet,
    Inet6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefix {
    kind: Kind,
    payload_len: usize,
}

impl Prefix {
    pub fn parse(bytes: &[u8; PREFIX_LEN]) -> Result<Self, ProxyError> {
        if bytes[..SIGNATURE.len()] != SIGNATURE {
            return Err(ProxyError::BadSignature);
        }
        let version = bytes[12] >> 4;
        if version != VERSION {
            return Err(ProxyError::UnsupportedVersion(version));
        }
        let command = bytes[12] & 0x0F;
        let family = bytes[13] >> 4;
        let transport = bytes[13] & 0x0F;
        let payload_len = usize::from(u16::from_be_bytes([bytes[14], bytes[15]]));
        if payload_len > MAX_PAYLOAD_LEN {
            return Err(ProxyError::PayloadTooLarge(payload_len));
        }
        let kind = match command {
            COMMAND_LOCAL => Kind::Local,
            COMMAND_PROXY => match (family, transport) {
                (FAMILY_UNSPEC, _) => Kind::Local,
                (FAMILY_INET, TRANSPORT_STREAM) => Kind::Inet,
                (FAMILY_INET6, TRANSPORT_STREAM) => Kind::Inet6,
                _ => return Err(ProxyError::UnsupportedTransport),
            },
            other => return Err(ProxyError::UnsupportedCommand(other)),
        };
        let needed = match kind {
            Kind::Local => 0,
            Kind::Inet => INET_ADDRESS_LEN,
            Kind::Inet6 => INET6_ADDRESS_LEN,
        };
        if payload_len < needed {
            return Err(ProxyError::PayloadTooShort {
                needed,
                got: payload_len,
            });
        }
        Ok(Self { kind, payload_len })
    }

    pub fn payload_len(&self) -> usize {
        self.payload_len
    }

    pub fn decode(&self, payload: &[u8]) -> Result<ProxyHeader, ProxyError> {
        if payload.len() < self.payload_len {
            return Err(ProxyError::Truncated);
        }
        let (source, destination) = match self.kind {
            Kind::Local => return Ok(ProxyHeader::Local),
            Kind::Inet => (
                socket_v4(&payload[0..4], &payload[8..10])?,
                socket_v4(&payload[4..8], &payload[10..12])?,
            ),
            Kind::Inet6 => (
                socket_v6(&payload[0..16], &payload[32..34])?,
                socket_v6(&payload[16..32], &payload[34..36])?,
            ),
        };
        if source.ip().is_unspecified() {
            return Err(ProxyError::InvalidAddress);
        }
        Ok(ProxyHeader::Proxied {
            source,
            destination,
        })
    }
}

impl ProxyHeader {
    pub fn parse(bytes: &[u8]) -> Result<(Self, usize), ProxyError> {
        let Some(prefix_bytes) = bytes.first_chunk::<PREFIX_LEN>() else {
            return Err(ProxyError::Truncated);
        };
        let prefix = Prefix::parse(prefix_bytes)?;
        let end = PREFIX_LEN + prefix.payload_len();
        let payload = bytes.get(PREFIX_LEN..end).ok_or(ProxyError::Truncated)?;
        Ok((prefix.decode(payload)?, end))
    }
}

fn socket_v4(address: &[u8], port: &[u8]) -> Result<SocketAddr, ProxyError> {
    let octets: [u8; 4] = address.try_into().map_err(|_| ProxyError::Truncated)?;
    let port: [u8; 2] = port.try_into().map_err(|_| ProxyError::Truncated)?;
    Ok(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::from(octets)),
        u16::from_be_bytes(port),
    ))
}

fn socket_v6(address: &[u8], port: &[u8]) -> Result<SocketAddr, ProxyError> {
    let octets: [u8; 16] = address.try_into().map_err(|_| ProxyError::Truncated)?;
    let port: [u8; 2] = port.try_into().map_err(|_| ProxyError::Truncated)?;
    Ok(SocketAddr::new(
        IpAddr::V6(Ipv6Addr::from(octets)),
        u16::from_be_bytes(port),
    ))
}
