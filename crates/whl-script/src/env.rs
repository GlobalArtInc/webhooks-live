use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

pub trait MonotonicClock: Send + Sync {
    fn elapsed(&self) -> Duration;
}

pub trait RandomSource: Send {
    fn fill_bytes(&mut self, destination: &mut [u8]);
}

#[derive(Debug, Clone, Copy)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for SystemClock {
    fn elapsed(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct RandomSeed([u8; 32]);

impl RandomSeed {
    pub fn from_bytes(material: &[u8]) -> Self {
        Self(Sha256::digest(material).into())
    }

    pub fn derive(key: &[u8], run_id: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"whl-script-seed");
        hasher.update(u64::try_from(key.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(key);
        hasher.update(run_id);
        Self(hasher.finalize().into())
    }

    pub fn unpack(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for RandomSeed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RandomSeed(..)")
    }
}

#[derive(Clone)]
pub struct SeededRandom {
    seed: [u8; 32],
    counter: u64,
    block: [u8; 32],
    offset: usize,
}

impl SeededRandom {
    pub fn from_seed(seed: &RandomSeed) -> Self {
        Self {
            seed: *seed.unpack(),
            counter: 0,
            block: [0; 32],
            offset: 32,
        }
    }

    fn refill(&mut self) {
        let mut hasher = Sha256::new();
        hasher.update(self.seed);
        hasher.update(self.counter.to_le_bytes());
        self.block = hasher.finalize().into();
        self.counter = self.counter.wrapping_add(1);
        self.offset = 0;
    }
}

impl std::fmt::Debug for SeededRandom {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SeededRandom(..)")
    }
}

impl RandomSource for SeededRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        for byte in destination.iter_mut() {
            if self.offset >= self.block.len() {
                self.refill();
            }
            *byte = self.block[self.offset];
            self.offset += 1;
        }
    }
}
