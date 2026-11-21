use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::compiled::{CompiledScript, ScriptCompiler, ScriptHash};
use crate::error::ScriptError;
use crate::limits::LimitsError;
use crate::pool::ScriptPool;

const WEIGHT_PER_SOURCE_BYTE: usize = 64;
const WEIGHT_PER_ENTRY: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheConfig {
    pub max_entries: usize,
    pub max_weight_bytes: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            max_weight_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub entries: usize,
    pub weight_bytes: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

struct Entry {
    script: CompiledScript,
    weight: usize,
    tick: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<ScriptHash, Entry>,
    weight: usize,
    tick: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

pub struct ScriptCache {
    config: CacheConfig,
    state: Mutex<State>,
}

impl std::fmt::Debug for ScriptCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScriptCache")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

fn weight_of(script: &CompiledScript) -> usize {
    script
        .source_bytes()
        .saturating_mul(WEIGHT_PER_SOURCE_BYTE)
        .saturating_add(WEIGHT_PER_ENTRY)
}

impl ScriptCache {
    pub fn new(config: CacheConfig) -> Result<Self, LimitsError> {
        if config.max_entries == 0 {
            return Err(LimitsError::Zero("max_entries"));
        }
        if config.max_weight_bytes == 0 {
            return Err(LimitsError::Zero("max_weight_bytes"));
        }
        Ok(Self {
            config,
            state: Mutex::new(State::default()),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn get(&self, hash: &ScriptHash) -> Option<CompiledScript> {
        let mut state = self.lock();
        state.tick += 1;
        let tick = state.tick;
        match state.entries.get_mut(hash) {
            Some(entry) => {
                entry.tick = tick;
                let script = entry.script.clone();
                state.hits += 1;
                Some(script)
            }
            None => {
                state.misses += 1;
                None
            }
        }
    }

    pub fn insert(&self, script: CompiledScript) {
        let weight = weight_of(&script);
        if weight > self.config.max_weight_bytes {
            return;
        }
        let mut state = self.lock();
        state.tick += 1;
        let tick = state.tick;
        if let Some(previous) = state.entries.remove(&script.hash()) {
            state.weight = state.weight.saturating_sub(previous.weight);
        }
        while state.entries.len() >= self.config.max_entries
            || state.weight.saturating_add(weight) > self.config.max_weight_bytes
        {
            let oldest = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.tick)
                .map(|(hash, _)| *hash);
            let Some(oldest) = oldest else { break };
            if let Some(removed) = state.entries.remove(&oldest) {
                state.weight = state.weight.saturating_sub(removed.weight);
                state.evictions += 1;
            }
        }
        state.weight = state.weight.saturating_add(weight);
        state.entries.insert(
            script.hash(),
            Entry {
                script,
                weight,
                tick,
            },
        );
    }

    pub fn get_or_compile(
        &self,
        pool: &ScriptPool,
        compiler: &ScriptCompiler,
        source: &str,
    ) -> Result<CompiledScript, ScriptError> {
        let hash = ScriptHash::of(source);
        if let Some(found) = self
            .get(&hash)
            .filter(|found| found.limits() == compiler.limits())
        {
            return Ok(found);
        }
        let compiled = pool.compile(compiler, source)?;
        self.insert(compiled.clone());
        Ok(compiled)
    }

    pub fn stats(&self) -> CacheStats {
        let state = self.lock();
        CacheStats {
            entries: state.entries.len(),
            weight_bytes: state.weight,
            hits: state.hits,
            misses: state.misses,
            evictions: state.evictions,
        }
    }
}
