use std::time::{Duration, Instant};

use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use sha2::{Digest, Sha256};

pub trait MonotonicClock {
    fn elapsed(&self) -> Duration;
}

pub trait RandomSource {
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
        let digest = Sha256::digest(material);
        Self(digest.into())
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

#[derive(Debug, Clone)]
pub struct ChaChaRandom {
    generator: ChaCha20Rng,
}

impl ChaChaRandom {
    pub fn from_seed(seed: &RandomSeed) -> Self {
        Self {
            generator: ChaCha20Rng::from_seed(*seed.unpack()),
        }
    }
}

impl RandomSource for ChaChaRandom {
    fn fill_bytes(&mut self, destination: &mut [u8]) {
        self.generator.fill_bytes(destination);
    }
}

pub struct RenderEnv<'a> {
    pub random: &'a mut dyn RandomSource,
    pub clock: &'a dyn MonotonicClock,
}

impl<'a> RenderEnv<'a> {
    pub fn new(random: &'a mut dyn RandomSource, clock: &'a dyn MonotonicClock) -> Self {
        Self { random, clock }
    }
}
