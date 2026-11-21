use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use argon2::{Algorithm, Argon2, Params, Version};
use subtle::ConstantTimeEq;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use whl_core::entropy::random_bytes;
use whl_store::ViewPasswordHash;

pub const MIN_PASSWORD_CHARS: usize = 8;
pub const MAX_PASSWORD_CHARS: usize = 128;
pub const MEMORY_KIB: u32 = 19 * 1024;
pub const ITERATIONS: u32 = 2;
pub const LANES: u32 = 1;
pub const MAX_CONCURRENT_HASHES: usize = 2;
pub const MAX_CONCURRENT_VERIFIES: usize = 2;
pub const HASH_WAIT: Duration = Duration::from_secs(2);

const MAX_STORED_MEMORY_KIB: u32 = 64 * 1024;
const MAX_STORED_ITERATIONS: u32 = 4;
const MAX_STORED_LANES: u32 = 4;
const OUTPUT_BYTES: usize = 32;
const SALT_BYTES: usize = 16;
const ALGORITHM_IDENT: &str = "argon2id";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordShape {
    #[error("the password is shorter than 8 characters")]
    TooShort,
    #[error("the password is longer than 128 characters")]
    TooLong,
}

#[derive(Clone)]
pub struct ViewPassword(String);

impl ViewPassword {
    pub fn parse(text: &str) -> Result<Self, PasswordShape> {
        match shape_of(text) {
            Some(shape) => Err(shape),
            None => Ok(Self(text.to_owned())),
        }
    }

    pub fn unpack(&self) -> &str {
        &self.0
    }
}

pub(crate) fn shape_of(text: &str) -> Option<PasswordShape> {
    let counted = text.chars().take(MAX_PASSWORD_CHARS + 1).count();
    if counted < MIN_PASSWORD_CHARS {
        Some(PasswordShape::TooShort)
    } else if counted > MAX_PASSWORD_CHARS {
        Some(PasswordShape::TooLong)
    } else {
        None
    }
}

impl PartialEq for ViewPassword {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.0.as_bytes().ct_eq(other.0.as_bytes()))
    }
}

impl Eq for ViewPassword {}

impl fmt::Debug for ViewPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ViewPassword(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PasswordError {
    #[error("the password hashing capacity is exhausted")]
    Busy,
    #[error("the operating system random source failed")]
    Entropy,
    #[error("the stored password hash is not usable")]
    StoredHashInvalid,
    #[error("the password could not be hashed")]
    Hashing,
    #[error("the password hashing task did not finish")]
    Task,
}

#[derive(Debug, Clone)]
pub struct ViewPasswords {
    hashing: Arc<Semaphore>,
    verifying: Arc<Semaphore>,
    wait: Duration,
}

impl Default for ViewPasswords {
    fn default() -> Self {
        Self::with_capacity(MAX_CONCURRENT_HASHES, MAX_CONCURRENT_VERIFIES, HASH_WAIT)
    }
}

impl ViewPasswords {
    pub fn with_capacity(hashing: usize, verifying: usize, wait: Duration) -> Self {
        Self {
            hashing: Arc::new(Semaphore::new(hashing)),
            verifying: Arc::new(Semaphore::new(verifying)),
            wait,
        }
    }

    pub fn available_hash_permits(&self) -> usize {
        self.hashing.available_permits()
    }

    pub fn available_verify_permits(&self) -> usize {
        self.verifying.available_permits()
    }

    pub async fn hash(&self, password: &ViewPassword) -> Result<ViewPasswordHash, PasswordError> {
        let candidate = password.unpack().to_owned();
        self.run(&self.hashing, move || hash_blocking(&candidate))
            .await?
    }

    pub async fn verify(
        &self,
        candidate: &str,
        stored: &ViewPasswordHash,
    ) -> Result<bool, PasswordError> {
        let candidate = candidate.to_owned();
        let stored = stored.clone();
        self.run(&self.verifying, move || {
            verify_blocking(&candidate, stored.unpack())
        })
        .await?
    }

    async fn acquire(
        &self,
        permits: &Arc<Semaphore>,
    ) -> Result<OwnedSemaphorePermit, PasswordError> {
        match tokio::time::timeout(self.wait, Arc::clone(permits).acquire_owned()).await {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_closed)) => Err(PasswordError::Task),
            Err(_elapsed) => Err(PasswordError::Busy),
        }
    }

    async fn run<T, F>(&self, permits: &Arc<Semaphore>, job: F) -> Result<T, PasswordError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let permit = self.acquire(permits).await?;
        tokio::task::spawn_blocking(move || {
            let outcome = job();
            drop(permit);
            outcome
        })
        .await
        .map_err(|_| PasswordError::Task)
    }
}

fn engine(params: Params) -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
}

fn own_params() -> Result<Params, PasswordError> {
    Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(OUTPUT_BYTES))
        .map_err(|_| PasswordError::Hashing)
}

fn hash_blocking(password: &str) -> Result<ViewPasswordHash, PasswordError> {
    let salt = random_bytes::<SALT_BYTES>().map_err(|_| PasswordError::Entropy)?;
    let hashed: PasswordHash = engine(own_params()?)
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|_| PasswordError::Hashing)?;
    Ok(ViewPasswordHash::new(hashed.to_string()))
}

fn verify_blocking(candidate: &str, stored: &str) -> Result<bool, PasswordError> {
    let parsed = PasswordHash::new(stored).map_err(|_| PasswordError::StoredHashInvalid)?;
    if parsed.algorithm.as_str() != ALGORITHM_IDENT {
        return Err(PasswordError::StoredHashInvalid);
    }
    let params = Params::try_from(&parsed).map_err(|_| PasswordError::StoredHashInvalid)?;
    if params.m_cost() > MAX_STORED_MEMORY_KIB
        || params.t_cost() > MAX_STORED_ITERATIONS
        || params.p_cost() > MAX_STORED_LANES
    {
        return Err(PasswordError::StoredHashInvalid);
    }
    match engine(params).verify_password(candidate.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::PasswordInvalid) => Ok(false),
        Err(_) => Err(PasswordError::StoredHashInvalid),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "sample-view-phrase";
    const OTHER: &str = "another-view-phrase";

    fn password(text: &str) -> ViewPassword {
        ViewPassword::parse(text).expect("the sample password is within bounds")
    }

    #[test]
    fn the_length_bounds_count_characters_not_bytes() {
        assert_eq!(
            ViewPassword::parse("1234567").map(|_| ()),
            Err(PasswordShape::TooShort)
        );
        assert!(ViewPassword::parse("12345678").is_ok());
        assert!(ViewPassword::parse(&"é".repeat(MAX_PASSWORD_CHARS)).is_ok());
        assert_eq!(
            ViewPassword::parse(&"é".repeat(MAX_PASSWORD_CHARS + 1)).map(|_| ()),
            Err(PasswordShape::TooLong)
        );
    }

    #[test]
    fn the_debug_output_hides_the_password_and_the_hash() {
        assert!(!format!("{:?}", password(SAMPLE)).contains(SAMPLE));
        let hash = ViewPasswordHash::new("$argon2id$v=19$m=1,t=1,p=1$c2FsdA$aGFzaA".to_owned());
        assert!(!format!("{hash:?}").contains("argon2id"));
    }

    #[tokio::test]
    async fn a_hash_accepts_the_password_and_refuses_another() {
        let passwords = ViewPasswords::default();
        let hash = passwords.hash(&password(SAMPLE)).await.expect("hashes");
        assert!(hash.unpack().starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert_eq!(passwords.verify(SAMPLE, &hash).await, Ok(true));
        assert_eq!(passwords.verify(OTHER, &hash).await, Ok(false));
        let again = passwords.hash(&password(SAMPLE)).await.expect("hashes");
        assert_ne!(hash.unpack(), again.unpack());
    }

    #[tokio::test]
    async fn a_stored_hash_above_the_bounds_or_malformed_is_refused_before_hashing() {
        let passwords = ViewPasswords::default();
        for stored in [
            "$argon2id$v=19$m=4194304,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g",
            "$argon2id$v=19$m=19456,t=1000,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g",
            "$argon2id$v=19$m=19456,t=2,p=64$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g",
            "$argon2i$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g",
            "not a hash",
            "",
        ] {
            let outcome = passwords
                .verify(SAMPLE, &ViewPasswordHash::new(stored.to_owned()))
                .await;
            assert_eq!(outcome, Err(PasswordError::StoredHashInvalid), "{stored}");
        }
    }

    #[tokio::test]
    async fn hashing_and_verifying_are_bounded_by_separate_semaphores() {
        let passwords = ViewPasswords::with_capacity(1, 1, Duration::from_millis(50));
        let stored = ViewPasswordHash::new("$argon2id$v=19$m=1,t=1,p=1$c2FsdA$aGFzaA".to_owned());
        let hashing = passwords
            .acquire(&passwords.hashing)
            .await
            .expect("the only hashing permit is free");
        assert_eq!(passwords.available_hash_permits(), 0);
        assert_eq!(passwords.available_verify_permits(), 1);

        assert_eq!(
            passwords.hash(&password(SAMPLE)).await.map(|_| ()),
            Err(PasswordError::Busy)
        );
        assert_eq!(
            passwords.verify(SAMPLE, &stored).await,
            Err(PasswordError::StoredHashInvalid),
            "verification is not starved by hashing"
        );
        let verifying = passwords
            .acquire(&passwords.verifying)
            .await
            .expect("the only verifying permit is free");
        assert_eq!(
            passwords.verify(SAMPLE, &stored).await,
            Err(PasswordError::Busy)
        );
        drop((hashing, verifying));
        assert!(passwords.hash(&password(SAMPLE)).await.is_ok());
        assert_eq!(passwords.available_hash_permits(), 1);
    }
}
