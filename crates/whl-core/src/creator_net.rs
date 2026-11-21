use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use ipnet::{IpNet, Ipv4Net, Ipv6Net};

pub const IPV4_PREFIX: u8 = 24;
pub const IPV6_PREFIX: u8 = 56;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum CreatorNetError {
    #[error("not a network address")]
    Unparsable,
    #[error("the prefix length is not the one used for creator networks")]
    WrongPrefix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CreatorNet(IpNet);

impl CreatorNet {
    pub fn from_ip(ip: IpAddr) -> Self {
        match ip.to_canonical() {
            IpAddr::V4(address) => Self(IpNet::V4(truncate_v4(address))),
            IpAddr::V6(address) => Self(IpNet::V6(truncate_v6(address))),
        }
    }

    pub fn unpack(&self) -> IpNet {
        self.0
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        self.0.contains(&ip.to_canonical())
    }
}

fn truncate_v4(address: Ipv4Addr) -> Ipv4Net {
    Ipv4Net::new(address, IPV4_PREFIX).map_or_else(|_| Ipv4Net::default(), |net| net.trunc())
}

fn truncate_v6(address: Ipv6Addr) -> Ipv6Net {
    Ipv6Net::new(address, IPV6_PREFIX).map_or_else(|_| Ipv6Net::default(), |net| net.trunc())
}

impl fmt::Display for CreatorNet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for CreatorNet {
    type Err = CreatorNetError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let net = IpNet::from_str(text).map_err(|_| CreatorNetError::Unparsable)?;
        let expected = match net {
            IpNet::V4(_) => IPV4_PREFIX,
            IpNet::V6(_) => IPV6_PREFIX,
        };
        if net.prefix_len() != expected || net != net.trunc() {
            return Err(CreatorNetError::WrongPrefix);
        }
        Ok(Self(net))
    }
}
