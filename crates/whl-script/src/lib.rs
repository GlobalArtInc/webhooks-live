#![deny(unsafe_code)]

mod alloc;
mod cache;
mod catalogue;
mod compiled;
mod convert;
mod engine;
mod env;
mod error;
mod exec;
mod guard;
mod host;
mod limits;
mod output;
mod pool;

#[cfg(test)]
mod stuck_tests;
#[cfg(test)]
mod surface_tests;
#[cfg(test)]
mod tests;

pub use alloc::{CountingAlloc, MemoryWindow, allocator_installed, memory_exceeded};
pub use cache::{CacheConfig, CacheStats, ScriptCache};
pub use catalogue::{
    FUNCTIONS, FunctionDoc, KEYWORDS, LANGUAGE_NAME, ParameterDoc, VARIABLES, VariableDoc,
};
pub use compiled::{CompiledScript, ScriptCompiler, ScriptHash};
pub use env::{MonotonicClock, RandomSeed, RandomSource, SeededRandom, SystemClock};
pub use error::{
    CompileError, Diagnostic, ErrorCode, LinePosition, MAX_MESSAGE_CHARS, Position,
    ResponseViolation, ScriptError,
};
pub use exec::{ScriptJob, TenantKey};
pub use limits::{
    AFTER_RESPONSE_OPERATIONS, AFTER_RESPONSE_TIME, BEFORE_RESPONSE_OPERATIONS,
    BEFORE_RESPONSE_TIME, DEADLINE_CHECK_INTERVAL, DEFAULT_LOG_BYTES, DEFAULT_MEMORY_BYTES,
    EngineLimits, LimitsError, MAX_CALL_LEVELS_CEILING, MAX_CONTAINER_ITEMS,
    MAX_EXPRESSION_DEPTH_CEILING, MAX_MAP_KEY_BYTES, MAX_SOURCE_BYTES, RunLimits, ScriptPhase,
};
pub use output::{
    MAX_CONTENT_TYPE_BYTES, MAX_HEADER_NAME_BYTES, MAX_HEADER_VALUE_BYTES, MAX_RESPONSE_HEADERS,
    ResponseHeader, RunStats, ScriptLog, ScriptOutput, ScriptResponse,
};
pub use pool::{Pending, PoolConfig, PoolError, PoolStats, ScriptPool};
