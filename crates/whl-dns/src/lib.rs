mod cache;
mod error;
mod ingress;
mod limits;
mod query;
mod responder;
mod server;
mod settings;
mod zone;

pub use cache::{EndpointCache, Standing};
pub use error::DnsError;
pub use ingress::{Ingress, IngressError, datagram_ingress, resolver_of};
pub use limits::{Admission, ConnectionGuard, ConnectionTable, NetworkKey, ResponseLimiter};
pub use query::{MAX_QUERY_BYTES, Parsed, Rejection, Request, parse};
pub use responder::{Outcome, Resolver, Responder, StorageProbe, Transport};
pub use server::{DnsHandle, DnsServer};
pub use settings::{DnsLimits, DnsSettings, SWEEP_INTERVAL};
pub use zone::{
    EDNS_UDP_LIMIT, PLAIN_UDP_LIMIT, Target, Zone, encode_within, truncated, udp_limit,
};
