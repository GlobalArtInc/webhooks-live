use std::fmt;

use zeroize::Zeroizing;

use crate::error::KeyIdError;

pub const KEY_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(u16);

impl KeyId {
    pub const MAX: u16 = i16::MAX as u16;

    pub fn new(id: u16) -> Result<Self, KeyIdError> {
        if (1..=Self::MAX).contains(&id) {
            Ok(Self(id))
        } else {
            Err(KeyIdError)
        }
    }

    pub fn unpack(self) -> u16 {
        self.0
    }

    pub fn from_column(value: i16) -> Result<Self, KeyIdError> {
        u16::try_from(value)
            .map_err(|_| KeyIdError)
            .and_then(Self::new)
    }

    pub fn to_column(self) -> i16 {
        i16::try_from(self.0).unwrap_or(i16::MAX)
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

pub(crate) struct MasterKey(Zeroizing<[u8; KEY_BYTES]>);

impl MasterKey {
    pub(crate) fn new(bytes: [u8; KEY_BYTES]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub(crate) fn unpack(&self) -> &[u8; KEY_BYTES] {
        &self.0
    }
}

impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MasterKey(<redacted>)")
    }
}
