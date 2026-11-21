use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use whl_core::entropy::{IdGenerationError, random_bytes};

pub const MAX_ADDRESS_BYTES: usize = 254;
pub const MAX_LOCAL_BYTES: usize = 64;
pub const MAX_LABEL_BYTES: usize = 63;
pub const TOKEN_BYTES: usize = 32;
pub const TOKEN_CHARS: usize = 43;

const LOCAL_SYMBOLS: &str = "._+-";
const GMAIL_DOMAINS: [&str; 2] = ["gmail.com", "googlemail.com"];
const GMAIL_CANONICAL: &str = "gmail.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("the address is empty")]
    Empty,
    #[error("the address is longer than {MAX_ADDRESS_BYTES} bytes")]
    TooLong,
    #[error("the address is not a plain ASCII mailbox of the form name@domain")]
    Format,
}

pub fn parse_address(text: &str) -> Result<String, AddressError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AddressError::Empty);
    }
    if text.len() > MAX_ADDRESS_BYTES {
        return Err(AddressError::TooLong);
    }
    let (local, domain) = text.split_once('@').ok_or(AddressError::Format)?;
    if !valid_local(local) || !valid_domain(domain) {
        return Err(AddressError::Format);
    }
    Ok(format!("{local}@{}", domain.to_ascii_lowercase()))
}

fn valid_local(local: &str) -> bool {
    !local.is_empty()
        && local.len() <= MAX_LOCAL_BYTES
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || LOCAL_SYMBOLS.contains(character))
}

fn valid_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 || !labels.iter().all(|label| valid_label(label)) {
        return false;
    }
    labels
        .last()
        .is_some_and(|top| top.len() >= 2 && !top.bytes().all(|byte| byte.is_ascii_digit()))
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= MAX_LABEL_BYTES
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressKey {
    pub mailbox: String,
    pub domain: String,
}

pub fn address_key(address: &str) -> AddressKey {
    let lowered = address.trim().to_ascii_lowercase();
    let (local, domain) = lowered.split_once('@').unwrap_or((lowered.as_str(), ""));
    let untagged = local.split('+').next().unwrap_or_default();
    let (local, domain) = if GMAIL_DOMAINS.contains(&domain) {
        (untagged.replace('.', ""), GMAIL_CANONICAL)
    } else {
        (untagged.to_owned(), domain)
    };
    AddressKey {
        mailbox: format!("{local}@{domain}"),
        domain: domain.to_owned(),
    }
}

pub fn address_digest(address: &str) -> String {
    let digest = Sha256::digest(address_key(address).mailbox.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Clone, PartialEq, Eq)]
pub struct VerificationToken(String);

impl VerificationToken {
    pub fn generate() -> Result<Self, IdGenerationError> {
        let bytes = random_bytes::<TOKEN_BYTES>()?;
        Ok(Self(URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn parse(text: &str) -> Option<Self> {
        let valid = text.len() == TOKEN_CHARS
            && text
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        valid.then(|| Self(text.to_owned()))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }

    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }
}

impl fmt::Debug for VerificationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerificationToken(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_mailbox_is_accepted_and_its_domain_is_lowercased() {
        assert_eq!(
            parse_address("  Jane.Doe+tag@Example.TEST ").as_deref(),
            Ok("Jane.Doe+tag@example.test")
        );
    }

    #[test]
    fn addresses_that_could_inject_or_route_elsewhere_are_refused() {
        for bad in [
            "a@b.test\r\nBcc: c@d.test",
            "a@b.test, c@d.test",
            "a b@c.test",
            "\"a b\"@c.test",
            "<a@b.test>",
            "a@[127.0.0.1]",
            "a@b",
            "a@b.123",
            "a@@b.test",
            "a@b..test",
            ".a@b.test",
            "a.@b.test",
            "a@-b.test",
            "\u{e9}@b.test",
            "a@b.t\u{e9}st",
            "",
        ] {
            assert!(parse_address(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn only_plain_local_part_characters_are_accepted() {
        for bad in [
            "a%b@c.test",
            "a!b@c.test",
            "a|b@c.test",
            "a/b@c.test",
            "a?b@c.test",
            "a{b@c.test",
            "a}b@c.test",
            "a'b@c.test",
            "a^b@c.test",
            "a$b@c.test",
            "a&b@c.test",
            "a*b@c.test",
            "a#b@c.test",
            "a~b@c.test",
            "a=b@c.test",
        ] {
            assert_eq!(parse_address(bad), Err(AddressError::Format), "{bad:?}");
        }
        assert!(parse_address("a.b_c+d-e@c.test").is_ok());
    }

    #[test]
    fn tags_and_gmail_dots_fold_into_one_key() {
        let plain = address_key("Vic.Tim+one@Example.Test");
        assert_eq!(plain.mailbox, "vic.tim@example.test");
        assert_eq!(plain.domain, "example.test");
        let first = address_key("v.i.c+1@gmail.com");
        assert_eq!(first, address_key("vic+2@googlemail.com"));
        assert_eq!(first.mailbox, "vic@gmail.com");
        assert_eq!(address_digest("a+x@b.test"), address_digest("A+y@B.test"));
    }

    #[test]
    fn an_address_longer_than_the_limit_is_refused() {
        let long = format!("{}@example.test", "a".repeat(MAX_LOCAL_BYTES + 1));
        assert_eq!(parse_address(&long), Err(AddressError::Format));
        let huge = format!("a@{}.test", "b".repeat(MAX_ADDRESS_BYTES));
        assert_eq!(parse_address(&huge), Err(AddressError::TooLong));
    }

    #[test]
    fn a_token_is_43_url_safe_characters_and_never_printed() {
        let token = VerificationToken::generate().expect("a token is generated");
        assert_eq!(token.unpack().len(), TOKEN_CHARS);
        assert!(VerificationToken::parse(token.unpack()).is_some());
        assert!(VerificationToken::parse("short").is_none());
        assert!(!format!("{token:?}").contains(token.unpack()));
        assert_eq!(token.digest(), token.digest());
    }
}
