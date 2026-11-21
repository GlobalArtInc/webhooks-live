use std::time::Duration;

use thiserror::Error;

pub const MAX_SOURCE_BYTES: usize = 16 * 1024;
pub const BEFORE_RESPONSE_OPERATIONS: u64 = 100_000;
pub const AFTER_RESPONSE_OPERATIONS: u64 = 1_000_000;
pub const BEFORE_RESPONSE_TIME: Duration = Duration::from_millis(100);
pub const AFTER_RESPONSE_TIME: Duration = Duration::from_secs(1);
pub const DEFAULT_MEMORY_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_LOG_BYTES: usize = 4 * 1024;
pub const DEADLINE_CHECK_INTERVAL: u64 = 16;
pub const MAX_MAP_KEY_BYTES: usize = 256;
pub const MAX_CONTAINER_ITEMS: usize = 4096;
pub const MAX_CALL_LEVELS_CEILING: usize = 64;
pub const MAX_EXPRESSION_DEPTH_CEILING: usize = 128;
pub const MAX_COMPILE_MEMORY_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LimitsError {
    #[error("limit {0} must be greater than zero")]
    Zero(&'static str),
    #[error("limit {name} must not exceed {ceiling}")]
    AboveCeiling { name: &'static str, ceiling: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptPhase {
    BeforeResponse,
    AfterResponse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineLimits {
    pub max_source_bytes: usize,
    pub max_call_levels: usize,
    pub max_expression_depth: usize,
    pub max_function_expression_depth: usize,
    pub max_string_bytes: usize,
    pub max_array_size: usize,
    pub max_map_size: usize,
    pub max_map_key_bytes: usize,
    pub max_functions: usize,
    pub max_variables: usize,
}

impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: MAX_SOURCE_BYTES,
            max_call_levels: 32,
            max_expression_depth: 64,
            max_function_expression_depth: 32,
            max_string_bytes: 16 * 1024,
            max_array_size: MAX_CONTAINER_ITEMS,
            max_map_size: MAX_CONTAINER_ITEMS,
            max_map_key_bytes: MAX_MAP_KEY_BYTES,
            max_functions: 32,
            max_variables: 128,
        }
    }
}

fn nonzero(name: &'static str, value: u64) -> Result<(), LimitsError> {
    if value == 0 {
        Err(LimitsError::Zero(name))
    } else {
        Ok(())
    }
}

fn ceiling(name: &'static str, value: u64, ceiling: u64) -> Result<(), LimitsError> {
    nonzero(name, value)?;
    if value > ceiling {
        Err(LimitsError::AboveCeiling { name, ceiling })
    } else {
        Ok(())
    }
}

fn widen(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

impl EngineLimits {
    pub fn validate(&self) -> Result<(), LimitsError> {
        ceiling("max_source_bytes", widen(self.max_source_bytes), 1 << 20)?;
        ceiling(
            "max_call_levels",
            widen(self.max_call_levels),
            widen(MAX_CALL_LEVELS_CEILING),
        )?;
        ceiling(
            "max_expression_depth",
            widen(self.max_expression_depth),
            widen(MAX_EXPRESSION_DEPTH_CEILING),
        )?;
        ceiling(
            "max_function_expression_depth",
            widen(self.max_function_expression_depth),
            widen(MAX_EXPRESSION_DEPTH_CEILING),
        )?;
        ceiling("max_string_bytes", widen(self.max_string_bytes), 1 << 20)?;
        ceiling("max_array_size", widen(self.max_array_size), 1 << 16)?;
        ceiling("max_map_size", widen(self.max_map_size), 1 << 16)?;
        ceiling("max_map_key_bytes", widen(self.max_map_key_bytes), 1 << 12)?;
        ceiling("max_functions", widen(self.max_functions), 1024)?;
        ceiling("max_variables", widen(self.max_variables), 4096)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunLimits {
    pub max_operations: u64,
    pub time: Duration,
    pub memory_bytes: usize,
    pub log_bytes: usize,
    pub max_input_bytes: usize,
    pub max_vars_bytes: usize,
    pub max_response_body_bytes: usize,
    pub max_host_bytes: u64,
    pub max_host_text_bytes: usize,
    pub regex_size_limit_bytes: usize,
}

impl RunLimits {
    pub fn before_response() -> Self {
        Self::for_phase(ScriptPhase::BeforeResponse, BEFORE_RESPONSE_TIME)
    }

    pub fn after_response() -> Self {
        Self::for_phase(ScriptPhase::AfterResponse, AFTER_RESPONSE_TIME)
    }

    pub fn for_phase(phase: ScriptPhase, time: Duration) -> Self {
        let max_operations = match phase {
            ScriptPhase::BeforeResponse => BEFORE_RESPONSE_OPERATIONS,
            ScriptPhase::AfterResponse => AFTER_RESPONSE_OPERATIONS,
        };
        Self {
            max_operations,
            time,
            memory_bytes: DEFAULT_MEMORY_BYTES,
            log_bytes: DEFAULT_LOG_BYTES,
            max_input_bytes: 256 * 1024,
            max_vars_bytes: 16 * 1024,
            max_response_body_bytes: 64 * 1024,
            max_host_bytes: 4 * 1024 * 1024,
            max_host_text_bytes: 64 * 1024,
            regex_size_limit_bytes: 256 * 1024,
        }
    }

    pub fn validate(&self) -> Result<(), LimitsError> {
        nonzero("max_operations", self.max_operations)?;
        nonzero(
            "time",
            u64::try_from(self.time.as_nanos()).unwrap_or(u64::MAX),
        )?;
        ceiling("memory_bytes", widen(self.memory_bytes), 1 << 32)?;
        nonzero("log_bytes", widen(self.log_bytes))?;
        nonzero("max_input_bytes", widen(self.max_input_bytes))?;
        nonzero("max_vars_bytes", widen(self.max_vars_bytes))?;
        nonzero(
            "max_response_body_bytes",
            widen(self.max_response_body_bytes),
        )?;
        nonzero("max_host_bytes", self.max_host_bytes)?;
        nonzero("max_host_text_bytes", widen(self.max_host_text_bytes))?;
        ceiling(
            "regex_size_limit_bytes",
            widen(self.regex_size_limit_bytes),
            1 << 22,
        )?;
        Ok(())
    }
}

impl Default for RunLimits {
    fn default() -> Self {
        Self::before_response()
    }
}
