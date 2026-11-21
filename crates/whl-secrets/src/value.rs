use std::fmt;

use zeroize::Zeroizing;

use crate::compare::constant_time_eq;
use crate::error::ValueError;

pub const MAX_VALUE_BYTES: usize = 4096;
pub const PLACEHOLDER: &str = "[redacted]";

pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Result<Self, ValueError> {
        let value = Zeroizing::new(value);
        if value.is_empty() {
            return Err(ValueError::Empty);
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(ValueError::TooLong);
        }
        Ok(Self(value))
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(PLACEHOLDER)
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(PLACEHOLDER)
    }
}

impl PartialEq for Secret {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(self.0.as_bytes(), other.0.as_bytes())
    }
}

impl Eq for Secret {}
