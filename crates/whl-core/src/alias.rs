use std::fmt;
use std::time::Duration;

pub const MIN_ALIAS_BYTES: usize = 3;
pub const MAX_ALIAS_BYTES: usize = 23;

pub const POSITIVE_CACHE_TTL: Duration = Duration::from_secs(30);

pub const RESERVED_ALIASES: [&str; 36] = [
    "a",
    "abuse",
    "admin",
    "api",
    "assets",
    "auth",
    "billing",
    "dns",
    "docs",
    "e",
    "endpoints",
    "ftp",
    "healthz",
    "help",
    "hostmaster",
    "imap",
    "login",
    "logout",
    "mail",
    "metrics",
    "new",
    "noc",
    "ns1",
    "ns2",
    "postmaster",
    "readyz",
    "root",
    "security",
    "settings",
    "signin",
    "smtp",
    "static",
    "status",
    "support",
    "webmaster",
    "www",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AliasError {
    #[error(
        "must be 3 to 23 characters of a to z, 0 to 9 and hyphen, starting with a letter and not ending with a hyphen"
    )]
    Format,
    #[error("is reserved")]
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Alias(String);

impl Alias {
    pub fn parse(text: &str) -> Result<Self, AliasError> {
        if RESERVED_ALIASES.contains(&text) {
            return Err(AliasError::Reserved);
        }
        Self::parse_shape(text)
    }

    pub fn parse_shape(text: &str) -> Result<Self, AliasError> {
        let bytes = text.as_bytes();
        let valid = (MIN_ALIAS_BYTES..=MAX_ALIAS_BYTES).contains(&bytes.len())
            && bytes[0].is_ascii_lowercase()
            && bytes[bytes.len() - 1] != b'-'
            && bytes
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(AliasError::Format)
        }
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Alias {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alias_takes_lowercase_letters_digits_and_inner_hyphens() {
        assert_eq!(
            Alias::parse("my-hook2").expect("valid").unpack(),
            "my-hook2"
        );
        for bad in [
            "My-hook", "1hook", "hook-", "-hook", "ho_ok", "hoo k", "höök",
        ] {
            assert_eq!(Alias::parse(bad), Err(AliasError::Format), "{bad}");
        }
    }

    #[test]
    fn an_alias_is_between_three_and_twenty_three_characters() {
        assert!(Alias::parse("abc").is_ok());
        assert!(Alias::parse(&"a".repeat(23)).is_ok());
        assert_eq!(Alias::parse("ab"), Err(AliasError::Format));
        assert_eq!(Alias::parse(&"a".repeat(24)), Err(AliasError::Format));
        assert_eq!(Alias::parse(""), Err(AliasError::Format));
    }

    #[test]
    fn every_reserved_name_is_refused_as_reserved() {
        for name in RESERVED_ALIASES {
            assert_eq!(Alias::parse(name), Err(AliasError::Reserved), "{name}");
        }
        assert!(Alias::parse("admins").is_ok());
    }

    #[test]
    fn the_reserved_list_covers_mailbox_and_service_names_without_duplicates() {
        for name in [
            "postmaster",
            "abuse",
            "hostmaster",
            "webmaster",
            "security",
            "root",
            "noc",
            "support",
            "billing",
            "metrics",
            "healthz",
            "readyz",
            "smtp",
            "imap",
            "ftp",
            "ns1",
            "ns2",
        ] {
            assert_eq!(Alias::parse(name), Err(AliasError::Reserved), "{name}");
        }
        let unique: std::collections::HashSet<&str> = RESERVED_ALIASES.into_iter().collect();
        assert_eq!(unique.len(), RESERVED_ALIASES.len());
    }
}
