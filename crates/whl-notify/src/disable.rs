use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use whl_secrets::{KeyId, Keyring, MAC_BYTES, MacError};
use whl_store::Uuid;

pub const MAX_DISABLE_TOKEN_BYTES: usize = 128;

const PURPOSE: &[u8] = b"whl-notify-disable-v1";
const CHANNEL_HEX_CHARS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DisableTokenError {
    #[error("cannot sign the disable token: {0}")]
    Sign(#[from] MacError),
    #[error("the disable token is not valid")]
    Invalid,
}

fn signed_input(channel: Uuid) -> Vec<u8> {
    let mut input = Vec::with_capacity(PURPOSE.len() + 16);
    input.extend_from_slice(PURPOSE);
    input.extend_from_slice(channel.as_bytes());
    input
}

pub fn issue_disable_token(keyring: &Keyring, channel: Uuid) -> Result<String, DisableTokenError> {
    let key_id = keyring.active_id();
    let key = keyring.notify_disable_mac_key(key_id)?;
    let tag = key.tag(&signed_input(channel))?;
    Ok(format!(
        "{}.{}.{}",
        key_id,
        channel.simple(),
        URL_SAFE_NO_PAD.encode(tag.unpack())
    ))
}

pub fn verify_disable_token(keyring: &Keyring, token: &str) -> Result<Uuid, DisableTokenError> {
    if token.len() > MAX_DISABLE_TOKEN_BYTES {
        return Err(DisableTokenError::Invalid);
    }
    let parts: Vec<&str> = token.split('.').collect();
    let [key, channel, tag] = parts.as_slice() else {
        return Err(DisableTokenError::Invalid);
    };
    let key_id = key
        .parse::<u16>()
        .ok()
        .and_then(|id| KeyId::new(id).ok())
        .filter(|id| keyring.contains(*id))
        .ok_or(DisableTokenError::Invalid)?;
    if channel.len() != CHANNEL_HEX_CHARS || !channel.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DisableTokenError::Invalid);
    }
    let channel = Uuid::try_parse(channel).map_err(|_| DisableTokenError::Invalid)?;
    let tag = URL_SAFE_NO_PAD
        .decode(tag)
        .ok()
        .filter(|bytes| bytes.len() == MAC_BYTES)
        .ok_or(DisableTokenError::Invalid)?;
    let mac_key = keyring.notify_disable_mac_key(key_id)?;
    if mac_key.verify(&signed_input(channel), &tag) {
        Ok(channel)
    } else {
        Err(DisableTokenError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use base64::engine::general_purpose::STANDARD;

    use super::*;

    fn keyring(entries: &[(u16, u8)]) -> Keyring {
        let spec = entries
            .iter()
            .map(|(id, fill)| format!("{id}:{}", STANDARD.encode([*fill; 32])))
            .collect::<Vec<_>>()
            .join(",");
        Keyring::parse(&spec).expect("a valid key set")
    }

    #[test]
    fn a_token_names_its_channel_and_survives_a_key_rotation() {
        let channel = Uuid::now_v7();
        let token = issue_disable_token(&keyring(&[(1, 1)]), channel).expect("a token");
        assert_eq!(
            verify_disable_token(&keyring(&[(1, 1)]), &token),
            Ok(channel)
        );
        assert_eq!(
            verify_disable_token(&keyring(&[(2, 2), (1, 1)]), &token),
            Ok(channel)
        );
    }

    #[test]
    fn a_forged_or_foreign_token_is_refused() {
        let channel = Uuid::now_v7();
        let ring = keyring(&[(1, 1)]);
        let token = issue_disable_token(&ring, channel).expect("a token");
        let other_channel = format!(
            "1.{}.{}",
            Uuid::now_v7().simple(),
            token.rsplit('.').next().expect("a tag")
        );
        let last = if token.ends_with('A') { 'B' } else { 'A' };
        let flipped = format!("{}{last}", &token[..token.len() - 1]);
        let foreign = issue_disable_token(&keyring(&[(1, 3)]), channel).expect("a token");
        for bad in [
            other_channel.as_str(),
            flipped.as_str(),
            foreign.as_str(),
            "",
            "1.2.3",
            "9.00000000000000000000000000000000.AAAA",
            &"x".repeat(MAX_DISABLE_TOKEN_BYTES + 1),
        ] {
            assert_eq!(
                verify_disable_token(&ring, bad),
                Err(DisableTokenError::Invalid),
                "{bad:?}"
            );
        }
    }
}
