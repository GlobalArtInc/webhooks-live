use crate::key::KeyId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a key id must be between 1 and {}", KeyId::MAX)]
pub struct KeyIdError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum KeyringProblem {
    #[error("the entry is not written as id:base64")]
    MissingSeparator,
    #[error("the key id is not a number between 1 and {}", KeyId::MAX)]
    InvalidId,
    #[error("the key id {0} is listed twice")]
    DuplicateId(KeyId),
    #[error("the key is not standard base64")]
    InvalidBase64,
    #[error("the key must decode to exactly 32 bytes")]
    WrongLength,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyringError {
    #[error("the key set is empty")]
    Empty,
    #[error("key set entry {position} is invalid: {problem}")]
    Entry {
        position: usize,
        problem: KeyringProblem,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("a secret name must match ^[A-Z][A-Z0-9_]{{0,63}}$")]
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ValueError {
    #[error("a secret value must not be empty")]
    Empty,
    #[error(
        "a secret value must be at most {} bytes",
        crate::value::MAX_VALUE_BYTES
    )]
    TooLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SealedError {
    #[error("the nonce must be {} bytes", crate::seal::NONCE_BYTES)]
    NonceLength,
    #[error(
        "the ciphertext must be between {} and {} bytes",
        crate::seal::MIN_CIPHERTEXT_BYTES,
        crate::seal::MAX_CIPHERTEXT_BYTES
    )]
    CiphertextLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SealError {
    #[error("the operating system random source failed")]
    Entropy,
    #[error("the active key is not in the key set")]
    ActiveKeyMissing,
    #[error("the cipher rejected the key or the input")]
    Cipher,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("key {0} is not in the key set")]
    UnknownKey(KeyId),
    #[error("the sealed value did not authenticate")]
    Authentication,
    #[error("the opened value is not valid")]
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ResealError {
    #[error(transparent)]
    Open(#[from] OpenError),
    #[error(transparent)]
    Seal(#[from] SealError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MacError {
    #[error("key {0} is not in the key set")]
    UnknownKey(KeyId),
    #[error("the key derivation failed")]
    Derivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RedactError {
    #[error(
        "at most {} secrets can be redacted at once",
        crate::redact::MAX_REDACT_SECRETS
    )]
    TooManySecrets,
}
