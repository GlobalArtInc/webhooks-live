use rand::TryRngCore;
use rand::rngs::OsRng;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdGenerationError {
    #[error("the operating system random source failed")]
    Entropy,
    #[error("the system clock is before the Unix epoch or out of range")]
    Clock,
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], IdGenerationError> {
    let mut bytes = [0_u8; N];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| IdGenerationError::Entropy)?;
    Ok(bytes)
}
