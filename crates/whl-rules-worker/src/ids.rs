use sha2::{Digest, Sha256};
use uuid::Uuid;

const NAMESPACE: &[u8] = b"webhooks.live/rule-step/v1";
const VERSION_MASK: u8 = 0x0f;
const VERSION_EIGHT: u8 = 0x80;
const VARIANT_MASK: u8 = 0x3f;
const VARIANT_RFC: u8 = 0x80;

pub fn step_id(run: Uuid, position: usize) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(NAMESPACE);
    hasher.update(run.as_bytes());
    hasher.update(u32::try_from(position).unwrap_or(u32::MAX).to_be_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    for (slot, byte) in bytes.iter_mut().zip(digest.iter()) {
        *slot = *byte;
    }
    bytes[6] = (bytes[6] & VERSION_MASK) | VERSION_EIGHT;
    bytes[8] = (bytes[8] & VARIANT_MASK) | VARIANT_RFC;
    Uuid::from_bytes(bytes)
}

pub fn delivery_header(run: Uuid, position: usize) -> String {
    format!("{}:{position}", run.hyphenated())
}
