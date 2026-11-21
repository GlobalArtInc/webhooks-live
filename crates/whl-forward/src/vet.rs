use thiserror::Error;
use url::Host;

use crate::config::GuardConfig;
use crate::error::RefusalReason;
use crate::lookup::LookupHost;
use crate::resolver::{ResolveError, vet_host};
use crate::target::{TargetUrl, validate_target_url};

const MAX_DETAIL_CHARS: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TargetCheckError {
    #[error("target refused: {0}")]
    Refused(RefusalReason),
    #[error("the host does not resolve: {0}")]
    Unresolvable(String),
}

pub async fn vet_target(
    input: &str,
    config: &GuardConfig,
    lookup: &dyn LookupHost,
) -> Result<TargetUrl, TargetCheckError> {
    let target = validate_target_url(input, config).map_err(|error| {
        TargetCheckError::Refused(error.refusal().unwrap_or(RefusalReason::InvalidUrl))
    })?;
    let Some(Host::Domain(domain)) = target.unpack().host() else {
        return Ok(target);
    };
    let vetted = tokio::time::timeout(
        config.connect_timeout,
        vet_host(lookup, &config.allow, domain),
    )
    .await;
    match vetted {
        Err(_) => Err(TargetCheckError::Unresolvable(
            "the lookup timed out".to_owned(),
        )),
        Ok(Ok(_)) => Ok(target),
        Ok(Err(ResolveError::Refused)) => {
            Err(TargetCheckError::Refused(RefusalReason::RefusedAddress))
        }
        Ok(Err(ResolveError::NoAddresses)) => Err(TargetCheckError::Unresolvable(
            "the host has no addresses".to_owned(),
        )),
        Ok(Err(ResolveError::Lookup(message))) => Err(TargetCheckError::Unresolvable(
            message.chars().take(MAX_DETAIL_CHARS).collect(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;
    use crate::static_lookup::StaticLookup;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn lookup() -> StaticLookup {
        StaticLookup::new()
            .with_answer("public.test", &[ip("93.184.216.34")])
            .with_answer("private.test", &[ip("10.0.0.5")])
            .with_answer("empty.test", &[])
    }

    #[tokio::test]
    async fn accepts_a_host_with_a_public_address() {
        let target = vet_target(
            "https://public.test/hook",
            &GuardConfig::default(),
            &lookup(),
        )
        .await
        .unwrap();
        assert_eq!(target.unpack().host_str(), Some("public.test"));
    }

    #[tokio::test]
    async fn refuses_a_host_that_resolves_only_to_a_refused_address() {
        let error = vet_target("https://private.test/", &GuardConfig::default(), &lookup())
            .await
            .unwrap_err();
        assert_eq!(
            error,
            TargetCheckError::Refused(RefusalReason::RefusedAddress)
        );
    }

    #[tokio::test]
    async fn reports_unknown_and_empty_hosts_as_unresolvable() {
        for input in ["https://missing.test/", "https://empty.test/"] {
            let error = vet_target(input, &GuardConfig::default(), &lookup())
                .await
                .unwrap_err();
            assert!(
                matches!(error, TargetCheckError::Unresolvable(_)),
                "{input}"
            );
        }
    }

    #[tokio::test]
    async fn refuses_syntax_and_literal_problems_without_a_lookup() {
        let probe = lookup();
        let literal = vet_target("http://169.254.169.254/", &GuardConfig::default(), &probe)
            .await
            .unwrap_err();
        assert_eq!(
            literal,
            TargetCheckError::Refused(RefusalReason::RefusedAddress)
        );
        let scheme = vet_target("ftp://public.test/", &GuardConfig::default(), &probe)
            .await
            .unwrap_err();
        assert_eq!(scheme, TargetCheckError::Refused(RefusalReason::Scheme));
        assert_eq!(probe.calls(), 0);
    }
}
