use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const SIGNATURE: [u8; 12] = *b"\r\n\r\n\0\r\nQUIT\n";
pub const PREFIX_LEN: usize = 16;
pub const MAX_PAYLOAD_LEN: usize = 520;
pub const MAX_HEADER_LEN: usize = PREFIX_LEN + MAX_PAYLOAD_LEN;

const VERSION_2: u8 = 2;
const COMMAND_LOCAL: u8 = 0;
const COMMAND_PROXY: u8 = 1;
const FAMILY_UNSPEC: u8 = 0;
const FAMILY_INET: u8 = 1;
const FAMILY_INET6: u8 = 2;
const TRANSPORT_STREAM: u8 = 1;
const TRANSPORT_DGRAM: u8 = 2;
const INET_BLOCK_LEN: usize = 12;
const INET6_BLOCK_LEN: usize = 36;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProxyError {
    #[error("the PROXY header is shorter than its length field declares")]
    Truncated,
    #[error("the data does not start with the PROXY protocol v2 signature")]
    Signature,
    #[error("PROXY protocol version {0} is not supported")]
    Version(u8),
    #[error("PROXY command {0} is not supported")]
    Command(u8),
    #[error("the PROXY header declares {declared} payload bytes, more than {MAX_PAYLOAD_LEN}")]
    TooLong { declared: usize },
    #[error("address family and transport byte {0:#04x} is not supported")]
    Unsupported(u8),
    #[error("the PROXY address block is shorter than its family requires")]
    AddressBlock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxiedAddresses {
    pub source: SocketAddr,
    pub destination: SocketAddr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyHeader {
    Local,
    Proxied(ProxiedAddresses),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedHeader {
    pub header: ProxyHeader,
    pub consumed: usize,
}

pub fn header_len(prefix: &[u8]) -> Result<usize, ProxyError> {
    let Some(prefix) = prefix.get(..PREFIX_LEN) else {
        return Err(ProxyError::Truncated);
    };
    if prefix[..SIGNATURE.len()] != SIGNATURE {
        return Err(ProxyError::Signature);
    }
    let version = prefix[12] >> 4;
    if version != VERSION_2 {
        return Err(ProxyError::Version(version));
    }
    let declared = usize::from(u16::from_be_bytes([prefix[14], prefix[15]]));
    if declared > MAX_PAYLOAD_LEN {
        return Err(ProxyError::TooLong { declared });
    }
    Ok(PREFIX_LEN + declared)
}

pub fn parse(bytes: &[u8]) -> Result<ParsedHeader, ProxyError> {
    let total = header_len(bytes)?;
    let Some(header) = bytes.get(..total) else {
        return Err(ProxyError::Truncated);
    };
    let command = header[12] & 0x0f;
    let parsed = match command {
        COMMAND_LOCAL => ProxyHeader::Local,
        COMMAND_PROXY if header[13] >> 4 == FAMILY_UNSPEC => ProxyHeader::Local,
        COMMAND_PROXY => ProxyHeader::Proxied(addresses(header[13], &header[PREFIX_LEN..])?),
        other => return Err(ProxyError::Command(other)),
    };
    Ok(ParsedHeader {
        header: parsed,
        consumed: total,
    })
}

fn addresses(family_and_transport: u8, block: &[u8]) -> Result<ProxiedAddresses, ProxyError> {
    let transport = family_and_transport & 0x0f;
    if transport != TRANSPORT_STREAM && transport != TRANSPORT_DGRAM {
        return Err(ProxyError::Unsupported(family_and_transport));
    }
    match family_and_transport >> 4 {
        FAMILY_INET => {
            let block = block
                .get(..INET_BLOCK_LEN)
                .ok_or(ProxyError::AddressBlock)?;
            let source = Ipv4Addr::new(block[0], block[1], block[2], block[3]);
            let destination = Ipv4Addr::new(block[4], block[5], block[6], block[7]);
            Ok(ProxiedAddresses {
                source: SocketAddr::new(
                    IpAddr::V4(source),
                    u16::from_be_bytes([block[8], block[9]]),
                ),
                destination: SocketAddr::new(
                    IpAddr::V4(destination),
                    u16::from_be_bytes([block[10], block[11]]),
                ),
            })
        }
        FAMILY_INET6 => {
            let block = block
                .get(..INET6_BLOCK_LEN)
                .ok_or(ProxyError::AddressBlock)?;
            let mut source = [0_u8; 16];
            let mut destination = [0_u8; 16];
            source.copy_from_slice(&block[..16]);
            destination.copy_from_slice(&block[16..32]);
            Ok(ProxiedAddresses {
                source: SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(source)),
                    u16::from_be_bytes([block[32], block[33]]),
                ),
                destination: SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(destination)),
                    u16::from_be_bytes([block[34], block[35]]),
                ),
            })
        }
        _ => Err(ProxyError::Unsupported(family_and_transport)),
    }
}
