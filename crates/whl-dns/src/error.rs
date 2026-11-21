use std::io;
use std::net::SocketAddr;

use whl_core::limiter::LimiterError;

#[derive(Debug, thiserror::Error)]
pub enum DnsError {
    #[error("the DNS zone {name:?} is not valid: {reason}")]
    Zone { name: String, reason: &'static str },
    #[error("the DNS name server name {name:?} is not valid: {reason}")]
    NameServer { name: String, reason: &'static str },
    #[error("the SOA contact {email:?} is not a valid email address")]
    SoaEmail { email: String },
    #[error("the DNS rate limiters cannot be built: {0}")]
    Limiter(#[from] LimiterError),
    #[error("cannot bind the DNS {transport} socket on {addr}: {source}")]
    Bind {
        transport: &'static str,
        addr: SocketAddr,
        source: io::Error,
    },
}
