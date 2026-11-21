use std::time::Duration;

use crate::target::RenderTarget;

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_EXPRESSIONS: usize = 500;
pub const MAX_FILTERS_PER_EXPRESSION: usize = 16;
pub const MAX_PATH_SEGMENTS: usize = 16;
pub const MAX_LITERAL_BYTES: usize = 1024;
pub const MAX_DIAGNOSTICS: usize = 50;
pub const MAX_ARGUMENTS: usize = 8;
pub const MAX_IDENTIFIER_BYTES: usize = 64;
pub const MAX_KEY_BYTES: usize = 128;

pub const BODY_TEXT_BYTES: usize = 64 * 1024;
pub const JSON_BODY_BYTES: usize = 1024 * 1024;
pub const JSON_BODY_DEPTH: usize = 64;
pub const JSON_BODY_DEFAULT_BYTES: usize = 256 * 1024;
pub const JSON_NODES_DEFAULT: usize = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLimits {
    pub max_source_bytes: usize,
    pub max_expressions: usize,
    pub max_filters_per_expression: usize,
    pub max_path_segments: usize,
    pub max_literal_bytes: usize,
    pub max_diagnostics: usize,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: MAX_SOURCE_BYTES,
            max_expressions: MAX_EXPRESSIONS,
            max_filters_per_expression: MAX_FILTERS_PER_EXPRESSION,
            max_path_segments: MAX_PATH_SEGMENTS,
            max_literal_bytes: MAX_LITERAL_BYTES,
            max_diagnostics: MAX_DIAGNOSTICS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderLimits {
    pub max_output_bytes: usize,
    pub max_evaluations: usize,
    pub deadline: Duration,
    pub max_value_bytes: usize,
    pub max_json_depth: usize,
    pub max_json_lookups: usize,
    pub max_json_nodes: usize,
    pub max_json_body_bytes: usize,
    pub max_errors: usize,
}

impl RenderLimits {
    pub fn for_target(target: RenderTarget) -> Self {
        Self {
            max_output_bytes: target.default_output_limit(),
            max_evaluations: 5000,
            deadline: Duration::from_millis(50),
            max_value_bytes: 1024 * 1024,
            max_json_depth: MAX_PATH_SEGMENTS,
            max_json_lookups: 500,
            max_json_nodes: JSON_NODES_DEFAULT,
            max_json_body_bytes: JSON_BODY_DEFAULT_BYTES,
            max_errors: MAX_DIAGNOSTICS,
        }
    }
}
