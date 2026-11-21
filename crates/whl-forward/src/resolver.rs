use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use ipnet::IpNet;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use thiserror::Error;

use crate::lookup::LookupHost;
use crate::ranges::is_refused;

pub const MAX_VETTED_ADDRESSES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum ResolveError {
    #[error("every resolved address is in a refused range")]
    Refused,
    #[error("host has no addresses")]
    NoAddresses,
    #[error("{0}")]
    Lookup(String),
}

pub(crate) async fn vet_host(
    lookup: &dyn LookupHost,
    allow: &[IpNet],
    host: &str,
) -> Result<Vec<IpAddr>, ResolveError> {
    if let Ok(literal) = host.parse::<IpAddr>() {
        return if is_refused(literal, allow) {
            Err(ResolveError::Refused)
        } else {
            Ok(vec![literal])
        };
    }
    let resolved = lookup
        .lookup(host)
        .await
        .map_err(|error| ResolveError::Lookup(error.to_string()))?;
    if resolved.is_empty() {
        return Err(ResolveError::NoAddresses);
    }
    let vetted: Vec<IpAddr> = resolved
        .into_iter()
        .filter(|address| !is_refused(*address, allow))
        .take(MAX_VETTED_ADDRESSES)
        .collect();
    if vetted.is_empty() {
        return Err(ResolveError::Refused);
    }
    Ok(vetted)
}

pub(crate) struct GuardedResolver {
    lookup: Arc<dyn LookupHost>,
    allow: Arc<[IpNet]>,
}

impl GuardedResolver {
    pub(crate) fn new(lookup: Arc<dyn LookupHost>, allow: &[IpNet]) -> Self {
        Self {
            lookup,
            allow: allow.into(),
        }
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let lookup = Arc::clone(&self.lookup);
        let allow = Arc::clone(&self.allow);
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let vetted = vet_host(lookup.as_ref(), &allow, &host).await?;
            let addresses: Addrs = Box::new(
                vetted
                    .into_iter()
                    .map(|address| SocketAddr::new(address, 0)),
            );
            Ok(addresses)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;
    use crate::static_lookup::StaticLookup;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[tokio::test]
    async fn keeps_only_vetted_addresses_of_a_mixed_answer() {
        let lookup = StaticLookup::new().with_answer(
            "mixed.test",
            &[
                ip("10.0.0.5"),
                ip("93.184.216.34"),
                ip("169.254.169.254"),
                ip("2606:4700:4700::1111"),
            ],
        );
        let vetted = vet_host(&lookup, &[], "mixed.test").await.unwrap();
        assert_eq!(
            vetted,
            vec![ip("93.184.216.34"), ip("2606:4700:4700::1111")]
        );
    }

    #[tokio::test]
    async fn refuses_a_host_whose_answers_are_all_refused() {
        let lookup = StaticLookup::new().with_answer("internal.test", &[ip("10.0.0.5"), ip("::1")]);
        let error = vet_host(&lookup, &[], "internal.test").await.unwrap_err();
        assert_eq!(error, ResolveError::Refused);
    }

    #[tokio::test]
    async fn does_not_resolve_literal_addresses() {
        let lookup = StaticLookup::new();
        assert_eq!(
            vet_host(&lookup, &[], "8.8.8.8").await.unwrap(),
            vec![ip("8.8.8.8")]
        );
        assert_eq!(
            vet_host(&lookup, &[], "127.0.0.1").await.unwrap_err(),
            ResolveError::Refused
        );
        assert_eq!(
            vet_host(&lookup, &[], "::ffff:10.0.0.1").await.unwrap_err(),
            ResolveError::Refused
        );
        assert_eq!(lookup.calls(), 0);
    }

    #[tokio::test]
    async fn allow_list_keeps_an_allowed_private_answer() {
        let lookup =
            StaticLookup::new().with_answer("local.test", &[ip("127.0.0.1"), ip("10.0.0.5")]);
        let allow = ["127.0.0.1/32".parse().unwrap()];
        let vetted = vet_host(&lookup, &allow, "local.test").await.unwrap();
        assert_eq!(vetted, vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    }

    #[tokio::test]
    async fn reports_lookup_failures_and_empty_answers() {
        let lookup = StaticLookup::new().with_answer("empty.test", &[]);
        assert!(matches!(
            vet_host(&lookup, &[], "missing.test").await,
            Err(ResolveError::Lookup(_))
        ));
        assert_eq!(
            vet_host(&lookup, &[], "empty.test").await.unwrap_err(),
            ResolveError::NoAddresses
        );
    }

    #[tokio::test]
    async fn caps_the_number_of_vetted_addresses() {
        let many: Vec<IpAddr> = (1..=64u8)
            .map(|last| IpAddr::V4(Ipv4Addr::new(8, 8, 8, last)))
            .collect();
        let lookup = StaticLookup::new().with_answer("many.test", &many);
        let vetted = vet_host(&lookup, &[], "many.test").await.unwrap();
        assert_eq!(vetted.len(), MAX_VETTED_ADDRESSES);
    }
}
