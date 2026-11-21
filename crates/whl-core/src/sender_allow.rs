use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use ipnet::{IpNet, Ipv4Net};

pub const MAX_SENDER_RANGES: usize = 20;
const MAX_RANGE_TEXT_BYTES: usize = 64;
const MAPPED_PREFIX_BITS: u8 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SenderRangeError {
    #[error("must be an IP address or a CIDR range")]
    Unparsable,
    #[error("must not have bits set beyond the prefix length")]
    HostBits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SenderRange(IpNet);

impl SenderRange {
    pub fn parse(text: &str) -> Result<Self, SenderRangeError> {
        if text.is_empty() || text.len() > MAX_RANGE_TEXT_BYTES {
            return Err(SenderRangeError::Unparsable);
        }
        let net = if text.contains('/') {
            let net = IpNet::from_str(text).map_err(|_| SenderRangeError::Unparsable)?;
            if net != net.trunc() {
                return Err(SenderRangeError::HostBits);
            }
            net
        } else {
            IpNet::from(IpAddr::from_str(text).map_err(|_| SenderRangeError::Unparsable)?)
        };
        Ok(Self(normalise(net)))
    }

    pub fn unpack(&self) -> IpNet {
        self.0
    }

    pub fn contains(&self, address: IpAddr) -> bool {
        self.0.contains(&address.to_canonical())
    }
}

impl fmt::Display for SenderRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

fn normalise(net: IpNet) -> IpNet {
    if let IpNet::V6(v6) = net
        && v6.prefix_len() >= MAPPED_PREFIX_BITS
        && let Some(mapped) = v6.addr().to_ipv4_mapped()
        && let Ok(v4) = Ipv4Net::new(mapped, v6.prefix_len() - MAPPED_PREFIX_BITS)
    {
        return IpNet::V4(v4);
    }
    net
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a sender allow list holds at most {MAX_SENDER_RANGES} ranges")]
pub struct TooManyRanges;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SenderAllowList(Vec<SenderRange>);

impl SenderAllowList {
    pub fn new(ranges: Vec<SenderRange>) -> Result<Self, TooManyRanges> {
        let mut unique: Vec<SenderRange> = Vec::with_capacity(ranges.len());
        for range in ranges {
            if !unique.contains(&range) {
                unique.push(range);
            }
        }
        if unique.len() > MAX_SENDER_RANGES {
            return Err(TooManyRanges);
        }
        Ok(Self(unique))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn ranges(&self) -> &[SenderRange] {
        &self.0
    }

    pub fn permits(&self, address: IpAddr) -> bool {
        self.0.is_empty() || self.0.iter().any(|range| range.contains(address))
    }
}
