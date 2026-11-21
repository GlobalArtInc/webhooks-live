use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use whl_core::entropy::{IdGenerationError, random_bytes};

const TOKEN_BYTES: usize = 32;
const TOKEN_CHARS: usize = 43;

#[derive(Clone, PartialEq, Eq)]
pub struct RandomToken(String);

impl RandomToken {
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

    pub fn digest(&self) -> TokenHash {
        TokenHash(Sha256::digest(self.0.as_bytes()).into())
    }
}

impl fmt::Debug for RandomToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RandomToken(<redacted>)")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    pub fn unpack(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TokenHash(<redacted>)")
    }
}

pub fn login_key(state: &str, binding: &RandomToken) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"whl-login-v1");
    hasher.update([0_u8]);
    hasher.update(state.as_bytes());
    hasher.update([0_u8]);
    hasher.update(binding.unpack().as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_distinct_and_parse_back() {
        let first = RandomToken::generate().expect("entropy is available");
        let second = RandomToken::generate().expect("entropy is available");
        assert_ne!(first, second);
        assert_eq!(first.unpack().len(), TOKEN_CHARS);
        assert_eq!(RandomToken::parse(first.unpack()), Some(first));
    }

    #[test]
    fn parse_rejects_wrong_length_and_alphabet() {
        assert_eq!(RandomToken::parse(""), None);
        assert_eq!(RandomToken::parse(&"a".repeat(42)), None);
        assert_eq!(RandomToken::parse(&"a".repeat(44)), None);
        assert_eq!(RandomToken::parse(&format!("{}=", "a".repeat(42))), None);
        assert_eq!(RandomToken::parse(&format!("{}/", "a".repeat(42))), None);
        assert!(RandomToken::parse(&"a".repeat(43)).is_some());
    }

    #[test]
    fn digest_is_stable_and_differs_per_token() {
        let token = RandomToken::parse(&"a".repeat(43)).expect("valid token");
        let other = RandomToken::parse(&"b".repeat(43)).expect("valid token");
        assert_eq!(token.digest(), token.digest());
        assert_ne!(token.digest(), other.digest());
        assert_eq!(token.digest().unpack().len(), 32);
    }

    #[test]
    fn login_key_depends_on_state_and_binding() {
        let binding = RandomToken::parse(&"a".repeat(43)).expect("valid token");
        let other = RandomToken::parse(&"b".repeat(43)).expect("valid token");
        let key = login_key("state", &binding);
        assert_eq!(key, login_key("state", &binding));
        assert_ne!(key, login_key("other", &binding));
        assert_ne!(key, login_key("state", &other));
    }
}
