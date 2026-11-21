use whl_store::notify::ChannelKind;

pub const MAX_CHANNEL_URL_BYTES: usize = 512;
pub const MAX_CHAT_ID_BYTES: usize = 64;
pub const MAX_TOKEN_BYTES: usize = 128;

pub const SLACK_HOST: &str = "hooks.slack.com";
pub const DISCORD_HOSTS: [&str; 2] = ["discord.com", "discordapp.com"];
pub const TELEGRAM_HOST: &str = "api.telegram.org";

const SLACK_PREFIX: &str = "https://hooks.slack.com/services/";
const DISCORD_PREFIXES: [&str; 2] = [
    "https://discord.com/api/webhooks/",
    "https://discordapp.com/api/webhooks/",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TargetRefusal {
    #[error("the URL is longer than {MAX_CHANNEL_URL_BYTES} bytes")]
    TooLong,
    #[error("the URL does not match the pattern of this channel kind")]
    Pattern,
    #[error("the value is not valid for this channel kind")]
    Format,
}

fn path_chars(text: &str, extra: &[char]) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || extra.contains(&c))
}

fn within_length(url: &str) -> Result<(), TargetRefusal> {
    if url.len() > MAX_CHANNEL_URL_BYTES {
        Err(TargetRefusal::TooLong)
    } else {
        Ok(())
    }
}

pub fn validate_slack_url(url: &str) -> Result<(), TargetRefusal> {
    within_length(url)?;
    match url.strip_prefix(SLACK_PREFIX) {
        Some(rest) if path_chars(rest, &['/']) => Ok(()),
        _ => Err(TargetRefusal::Pattern),
    }
}

pub fn validate_discord_url(url: &str) -> Result<(), TargetRefusal> {
    within_length(url)?;
    let rest = DISCORD_PREFIXES
        .iter()
        .find_map(|prefix| url.strip_prefix(prefix))
        .ok_or(TargetRefusal::Pattern)?;
    let (id, token) = rest.split_once('/').ok_or(TargetRefusal::Pattern)?;
    if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) && path_chars(token, &[]) {
        Ok(())
    } else {
        Err(TargetRefusal::Pattern)
    }
}

pub fn validate_chat_id(chat_id: &str) -> Result<(), TargetRefusal> {
    if chat_id.is_empty() || chat_id.len() > MAX_CHAT_ID_BYTES {
        return Err(TargetRefusal::Format);
    }
    let numeric = chat_id.strip_prefix('-').unwrap_or(chat_id);
    if !numeric.is_empty() && numeric.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(());
    }
    match chat_id.strip_prefix('@') {
        Some(name)
            if (5..=32).contains(&name.len())
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_') =>
        {
            Ok(())
        }
        _ => Err(TargetRefusal::Format),
    }
}

pub fn validate_bot_token(token: &str) -> Result<(), TargetRefusal> {
    if token.len() > MAX_TOKEN_BYTES {
        return Err(TargetRefusal::Format);
    }
    match token.split_once(':') {
        Some((id, secret))
            if !id.is_empty()
                && id.bytes().all(|byte| byte.is_ascii_digit())
                && path_chars(secret, &[]) =>
        {
            Ok(())
        }
        _ => Err(TargetRefusal::Format),
    }
}

pub const MASK_SUFFIX: &str = "/...";

pub fn is_masked(url: &str) -> bool {
    url.ends_with(MASK_SUFFIX)
}

pub fn targets_instance(url: &str, public_url: &str) -> bool {
    let (Ok(candidate), Ok(own)) = (url::Url::parse(url), url::Url::parse(public_url)) else {
        return false;
    };
    let host = |parsed: &url::Url| {
        parsed
            .host_str()
            .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
    };
    host(&candidate).is_some()
        && host(&candidate) == host(&own)
        && candidate.port_or_known_default() == own.port_or_known_default()
}

pub fn mask_url(kind: ChannelKind, url: &str) -> String {
    let keep = match kind {
        ChannelKind::Slack => SLACK_PREFIX.trim_end_matches('/'),
        ChannelKind::Discord => DISCORD_PREFIXES
            .iter()
            .find(|prefix| url.starts_with(**prefix))
            .map_or("https://discord.com/api/webhooks", |prefix| {
                prefix.trim_end_matches('/')
            }),
        _ => return mask_origin(url),
    };
    format!("{keep}{MASK_SUFFIX}")
}

fn mask_origin(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => match parsed.host_str() {
            Some(host) => format!("{}://{host}{MASK_SUFFIX}", parsed.scheme()),
            None => format!("...{MASK_SUFFIX}"),
        },
        Err(_) => format!("...{MASK_SUFFIX}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_exact_slack_pattern() {
        assert_eq!(
            validate_slack_url("https://hooks.slack.com/services/T0/B0/xyz_-"),
            Ok(())
        );
        for refused in [
            "http://hooks.slack.com/services/T0/B0/xyz",
            "https://hooks.slack.com.evil.test/services/T0/B0/xyz",
            "https://hooks.slack.com@evil.test/services/T0/B0/xyz",
            "https://hooks.slack.com/services/",
            "https://hooks.slack.com/services/T0/B0?x=1",
            "https://hooks.slack.com/services/T0/B0 ",
            "https://hooks.slack.com:444/services/T0/B0",
        ] {
            assert_eq!(
                validate_slack_url(refused),
                Err(TargetRefusal::Pattern),
                "{refused}"
            );
        }
    }

    #[test]
    fn accepts_only_the_exact_discord_pattern() {
        assert_eq!(
            validate_discord_url("https://discord.com/api/webhooks/123/tok_en-1"),
            Ok(())
        );
        assert_eq!(
            validate_discord_url("https://discordapp.com/api/webhooks/9/a"),
            Ok(())
        );
        for refused in [
            "https://discord.com/api/webhooks/abc/token",
            "https://discord.com/api/webhooks/123/",
            "https://discord.com/api/webhooks/123/tok/en",
            "https://cdn.discord.com/api/webhooks/123/token",
            "https://discord.com/api/webhooks/123/token?wait=true",
        ] {
            assert_eq!(
                validate_discord_url(refused),
                Err(TargetRefusal::Pattern),
                "{refused}"
            );
        }
    }

    #[test]
    fn bounds_the_url_length() {
        let long = format!("https://hooks.slack.com/services/{}", "a".repeat(600));
        assert_eq!(validate_slack_url(&long), Err(TargetRefusal::TooLong));
    }

    #[test]
    fn accepts_numeric_and_named_chat_ids_only() {
        for good in ["123456", "-1001234567890", "@my_channel"] {
            assert_eq!(validate_chat_id(good), Ok(()), "{good}");
        }
        for bad in ["", "-", "@ab", "12 34", "1/../2", "@a b c d e", "chat"] {
            assert_eq!(validate_chat_id(bad), Err(TargetRefusal::Format), "{bad}");
        }
    }

    #[test]
    fn a_bot_token_cannot_alter_the_request_path() {
        assert_eq!(validate_bot_token("123456:AAE-x_y"), Ok(()));
        for bad in [
            "123456",
            "abc:def",
            "123:abc/../x",
            "123:a?b",
            "123:a b",
            "",
        ] {
            assert_eq!(validate_bot_token(bad), Err(TargetRefusal::Format), "{bad}");
        }
    }

    #[test]
    fn a_url_on_the_instance_host_and_port_is_recognised() {
        let own = "https://whl.example";
        assert!(targets_instance("https://whl.example/abc", own));
        assert!(targets_instance("https://WHL.example.:443/abc", own));
        assert!(!targets_instance("https://whl.example:8443/abc", own));
        assert!(!targets_instance("https://other.example/abc", own));
    }

    #[test]
    fn masks_keep_only_the_public_part() {
        assert_eq!(
            mask_url(
                ChannelKind::Slack,
                "https://hooks.slack.com/services/T0/B0/secret"
            ),
            "https://hooks.slack.com/services/..."
        );
        assert_eq!(
            mask_url(ChannelKind::Webhook, "https://example.test/path?token=abc"),
            "https://example.test/..."
        );
    }
}
