use std::collections::BTreeMap;
use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::{AeadInOut, KeyInit, XChaCha20Poly1305, XNonce};
use whl_core::endpoint_id::EndpointId;
use whl_core::entropy::random_bytes;
use zeroize::Zeroizing;

use crate::error::{KeyringError, KeyringProblem, OpenError, ResealError, SealError};
use crate::key::{KEY_BYTES, KeyId, MasterKey};
use crate::name::SecretName;
use crate::seal::{NONCE_BYTES, SealedSecret, TAG_BYTES, associated_data};
use crate::value::Secret;

pub struct Keyring {
    active: KeyId,
    keys: BTreeMap<KeyId, MasterKey>,
}

impl Keyring {
    pub fn parse(spec: &str) -> Result<Self, KeyringError> {
        let entries: Vec<&str> = spec.split(',').map(str::trim).collect();
        if entries.iter().all(|entry| entry.is_empty()) {
            return Err(KeyringError::Empty);
        }
        let mut active = None;
        let mut keys = BTreeMap::new();
        for (index, entry) in entries.into_iter().enumerate() {
            let position = index + 1;
            let wrap = |problem| KeyringError::Entry { position, problem };
            let (id, key) = parse_entry(entry).map_err(wrap)?;
            if keys.insert(id, key).is_some() {
                return Err(wrap(KeyringProblem::DuplicateId(id)));
            }
            active.get_or_insert(id);
        }
        let active = active.ok_or(KeyringError::Empty)?;
        Ok(Self { active, keys })
    }

    pub fn active_id(&self) -> KeyId {
        self.active
    }

    pub fn key_ids(&self) -> Vec<KeyId> {
        self.keys.keys().copied().collect()
    }

    pub fn contains(&self, id: KeyId) -> bool {
        self.keys.contains_key(&id)
    }

    pub(crate) fn master(&self, id: KeyId) -> Option<&MasterKey> {
        self.keys.get(&id)
    }

    pub fn seal(
        &self,
        endpoint: &EndpointId,
        name: &SecretName,
        value: &Secret,
    ) -> Result<SealedSecret, SealError> {
        let key_id = self.active;
        let key = self.master(key_id).ok_or(SealError::ActiveKeyMissing)?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.unpack()).map_err(|_| SealError::Cipher)?;
        let nonce = random_bytes::<NONCE_BYTES>().map_err(|_| SealError::Entropy)?;
        let aad = associated_data(endpoint, name, key_id);
        let plaintext = value.unpack().as_bytes();
        let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len() + TAG_BYTES));
        buffer.extend_from_slice(plaintext);
        cipher
            .encrypt_in_place(&XNonce::from(nonce), &aad, &mut *buffer)
            .map_err(|_| SealError::Cipher)?;
        Ok(SealedSecret::assemble(
            key_id,
            nonce,
            std::mem::take(&mut *buffer),
        ))
    }

    pub fn open(
        &self,
        endpoint: &EndpointId,
        name: &SecretName,
        sealed: &SealedSecret,
    ) -> Result<Secret, OpenError> {
        let key_id = sealed.key_id();
        let key = self.master(key_id).ok_or(OpenError::UnknownKey(key_id))?;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.unpack()).map_err(|_| OpenError::Corrupt)?;
        let aad = associated_data(endpoint, name, key_id);
        let mut buffer = Zeroizing::new(Vec::with_capacity(sealed.ciphertext().len()));
        buffer.extend_from_slice(sealed.ciphertext());
        cipher
            .decrypt_in_place(&XNonce::from(*sealed.nonce()), &aad, &mut *buffer)
            .map_err(|_| OpenError::Authentication)?;
        let text = std::str::from_utf8(&buffer).map_err(|_| OpenError::Corrupt)?;
        Secret::new(text.to_owned()).map_err(|_| OpenError::Corrupt)
    }

    pub fn reseal(
        &self,
        endpoint: &EndpointId,
        name: &SecretName,
        sealed: &SealedSecret,
    ) -> Result<SealedSecret, ResealError> {
        let value = self.open(endpoint, name, sealed)?;
        Ok(self.seal(endpoint, name, &value)?)
    }
}

impl fmt::Debug for Keyring {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keyring")
            .field("active", &self.active)
            .field("key_ids", &self.key_ids())
            .finish_non_exhaustive()
    }
}

fn parse_entry(entry: &str) -> Result<(KeyId, MasterKey), KeyringProblem> {
    let (id, encoded) = entry
        .split_once(':')
        .ok_or(KeyringProblem::MissingSeparator)?;
    let id = parse_id(id.trim())?;
    let mut decoded = Zeroizing::new(Vec::with_capacity(encoded.len()));
    STANDARD
        .decode_vec(encoded.trim(), &mut decoded)
        .map_err(|_| KeyringProblem::InvalidBase64)?;
    let bytes =
        <[u8; KEY_BYTES]>::try_from(decoded.as_slice()).map_err(|_| KeyringProblem::WrongLength)?;
    Ok((id, MasterKey::new(bytes)))
}

fn parse_id(text: &str) -> Result<KeyId, KeyringProblem> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(KeyringProblem::InvalidId);
    }
    text.parse::<u16>()
        .ok()
        .and_then(|id| KeyId::new(id).ok())
        .ok_or(KeyringProblem::InvalidId)
}
