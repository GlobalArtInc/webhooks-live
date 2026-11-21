use std::fmt;

use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use whl_core::endpoint_id::EndpointId;
use zeroize::Zeroizing;

use crate::compare::constant_time_eq;
use crate::error::MacError;
use crate::key::{KEY_BYTES, KeyId};
use crate::keyring::Keyring;

const CAPTURE_AUTH_CONTEXT: &[u8] = b"whl-capture-auth";
const CONTEXT_SEPARATOR: u8 = 0;
pub const MAC_BYTES: usize = 32;

pub struct MacKey(Zeroizing<[u8; KEY_BYTES]>);

#[derive(Clone)]
pub struct MacTag([u8; MAC_BYTES]);

impl Keyring {
    pub fn capture_mac_key(
        &self,
        key_id: KeyId,
        endpoint: &EndpointId,
    ) -> Result<MacKey, MacError> {
        let master = self.master(key_id).ok_or(MacError::UnknownKey(key_id))?;
        let mut info = Vec::with_capacity(CAPTURE_AUTH_CONTEXT.len() + 1 + endpoint.unpack().len());
        info.extend_from_slice(CAPTURE_AUTH_CONTEXT);
        info.push(CONTEXT_SEPARATOR);
        info.extend_from_slice(endpoint.unpack().as_bytes());
        let mut derived = Zeroizing::new([0_u8; KEY_BYTES]);
        Hkdf::<Sha256>::new(None, master.unpack())
            .expand(&info, &mut *derived)
            .map_err(|_| MacError::Derivation)?;
        Ok(MacKey(derived))
    }

    pub fn active_capture_mac_key(
        &self,
        endpoint: &EndpointId,
    ) -> Result<(KeyId, MacKey), MacError> {
        let key_id = self.active_id();
        Ok((key_id, self.capture_mac_key(key_id, endpoint)?))
    }
}

impl MacKey {
    pub fn tag(&self, value: &[u8]) -> Result<MacTag, MacError> {
        let mut mac = Hmac::<Sha256>::new_from_slice(&*self.0).map_err(|_| MacError::Derivation)?;
        mac.update(value);
        Ok(MacTag(mac.finalize().into_bytes().into()))
    }

    pub fn verify(&self, value: &[u8], expected: &[u8]) -> bool {
        self.tag(value)
            .is_ok_and(|tag| constant_time_eq(tag.unpack(), expected))
    }
}

impl MacTag {
    pub fn unpack(&self) -> &[u8; MAC_BYTES] {
        &self.0
    }
}

impl PartialEq for MacTag {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(&self.0, &other.0)
    }
}

impl Eq for MacTag {}

impl fmt::Debug for MacKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MacKey(<redacted>)")
    }
}

impl fmt::Debug for MacTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MacTag(<redacted>)")
    }
}
