use url::Url;

use crate::push::PushHosts;
use crate::target::{DISCORD_HOSTS, SLACK_HOST, TELEGRAM_HOST};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PinError {
    #[error("the URL is not valid")]
    Invalid,
    #[error("the URL does not use https on the default port without credentials")]
    Shape,
    #[error("the host {0:?} is not a pinned vendor host")]
    Host(String),
}

#[derive(Debug, Clone, Default)]
pub struct VendorHosts {
    push: PushHosts,
    #[cfg(any(test, feature = "test-support"))]
    overrides: Vec<(String, Url)>,
}

impl VendorHosts {
    pub fn production() -> Self {
        Self::default()
    }

    pub fn with_push_hosts(mut self, push: PushHosts) -> Self {
        self.push = push;
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_override(mut self, host: &str, origin: Url) -> Self {
        self.overrides.push((host.to_owned(), origin));
        self
    }

    pub fn resolve(&self, url: &str) -> Result<Url, PinError> {
        let (parsed, host) = pinned_shape(url)?;
        if !is_pinned(&host) {
            return Err(PinError::Host(host));
        }
        Ok(self.redirect(parsed, &host))
    }

    pub fn resolve_push(&self, url: &str) -> Result<Url, PinError> {
        let (parsed, host) = pinned_shape(url)?;
        if !self.push.allows(&host) {
            return Err(PinError::Host(host));
        }
        Ok(self.redirect(parsed, &host))
    }

    #[cfg(any(test, feature = "test-support"))]
    fn redirect(&self, parsed: Url, host: &str) -> Url {
        let Some((_, origin)) = self.overrides.iter().find(|(name, _)| name == host) else {
            return parsed;
        };
        let mut target = origin.clone();
        target.set_path(parsed.path());
        target.set_query(parsed.query());
        target
    }

    #[cfg(not(any(test, feature = "test-support")))]
    fn redirect(&self, parsed: Url, _host: &str) -> Url {
        parsed
    }
}

fn pinned_shape(url: &str) -> Result<(Url, String), PinError> {
    let parsed = Url::parse(url).map_err(|_| PinError::Invalid)?;
    if parsed.scheme() != "https"
        || parsed.port().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(PinError::Shape);
    }
    let host = parsed.host_str().ok_or(PinError::Invalid)?.to_owned();
    Ok((parsed, host))
}

fn is_pinned(host: &str) -> bool {
    host == SLACK_HOST || host == TELEGRAM_HOST || DISCORD_HOSTS.contains(&host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_the_three_vendors_and_nothing_else() {
        let hosts = VendorHosts::production();
        for good in [
            "https://hooks.slack.com/services/T/B/x",
            "https://discord.com/api/webhooks/1/x",
            "https://discordapp.com/api/webhooks/1/x",
            "https://api.telegram.org/bot1:x/sendMessage",
        ] {
            assert!(hosts.resolve(good).is_ok(), "{good}");
        }
        for bad in [
            "https://evil.test/services/T/B/x",
            "https://hooks.slack.com.evil.test/x",
            "https://api.telegram.org:8443/bot1:x/sendMessage",
            "http://hooks.slack.com/services/T/B/x",
            "https://user@hooks.slack.com/services/T/B/x",
        ] {
            assert!(hosts.resolve(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn push_urls_resolve_only_on_the_push_list_and_chat_hosts_stay_apart() {
        let hosts = VendorHosts::production();
        assert!(
            hosts
                .resolve_push("https://fcm.googleapis.com/fcm/send/x")
                .is_ok()
        );
        assert!(
            hosts
                .resolve_push("https://hooks.slack.com/services/T/B/x")
                .is_err()
        );
        assert!(
            hosts
                .resolve("https://fcm.googleapis.com/fcm/send/x")
                .is_err()
        );
        assert!(hosts.resolve_push("https://evil.test/x").is_err());
        assert!(hosts.resolve_push("http://fcm.googleapis.com/x").is_err());
    }

    #[test]
    fn the_test_map_moves_the_origin_and_keeps_the_path() {
        let origin = Url::parse("http://127.0.0.1:9000").unwrap();
        let hosts = VendorHosts::production().with_override("hooks.slack.com", origin);
        let resolved = hosts
            .resolve("https://hooks.slack.com/services/T/B/x")
            .unwrap();
        assert_eq!(resolved.as_str(), "http://127.0.0.1:9000/services/T/B/x");
    }
}
