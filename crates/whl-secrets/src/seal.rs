use std::fmt;

use whl_core::endpoint_id::EndpointId;

use crate::error::SealedError;
use crate::key::KeyId;
use crate::name::SecretName;
use crate::value::MAX_VALUE_BYTES;

pub const NONCE_BYTES: usize = 24;
pub const TAG_BYTES: usize = 16;
pub const MAX_CIPHERTEXT_BYTES: usize = MAX_VALUE_BYTES + TAG_BYTES;
pub(crate) const MIN_CIPHERTEXT_BYTES: usize = TAG_BYTES + 1;

const AAD_SEPARATOR: u8 = 0;

#[derive(Clone, PartialEq, Eq)]
pub struct SealedSecret {
    key_id: KeyId,
    nonce: [u8; NONCE_BYTES],
    ciphertext: Vec<u8>,
}

impl SealedSecret {
    pub fn from_parts(
        key_id: KeyId,
        nonce: &[u8],
        ciphertext: Vec<u8>,
    ) -> Result<Self, SealedError> {
        let nonce = <[u8; NONCE_BYTES]>::try_from(nonce).map_err(|_| SealedError::NonceLength)?;
        if !(MIN_CIPHERTEXT_BYTES..=MAX_CIPHERTEXT_BYTES).contains(&ciphertext.len()) {
            return Err(SealedError::CiphertextLength);
        }
        Ok(Self {
            key_id,
            nonce,
            ciphertext,
        })
    }

    pub(crate) fn assemble(key_id: KeyId, nonce: [u8; NONCE_BYTES], ciphertext: Vec<u8>) -> Self {
        Self {
            key_id,
            nonce,
            ciphertext,
        }
    }

    pub fn key_id(&self) -> KeyId {
        self.key_id
    }

    pub fn nonce(&self) -> &[u8; NONCE_BYTES] {
        &self.nonce
    }

    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }
}

impl fmt::Debug for SealedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SealedSecret")
            .field("key_id", &self.key_id)
            .field("ciphertext_bytes", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

pub(crate) fn associated_data(endpoint: &EndpointId, name: &SecretName, key_id: KeyId) -> Vec<u8> {
    let endpoint = endpoint.unpack().as_bytes();
    let name = name.unpack().as_bytes();
    let mut data = Vec::with_capacity(endpoint.len() + 1 + name.len() + 2);
    data.extend_from_slice(endpoint);
    data.push(AAD_SEPARATOR);
    data.extend_from_slice(name);
    data.extend_from_slice(&key_id.unpack().to_be_bytes());
    data
}
