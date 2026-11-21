pub const MAX_RULES_PER_ENDPOINT: usize = 5;
pub const MAX_STEPS_PER_RULE: usize = 10;
pub const MAX_RULE_SET_BYTES: usize = 128 * 1024;
pub const MAX_RULE_NAME_CHARS: usize = 100;
pub const MAX_STEP_NAME_BYTES: usize = 32;
pub const MAX_VARIABLE_NAME_BYTES: usize = 64;
pub const MAX_CONDITION_DEPTH: usize = 4;
pub const MAX_CONDITION_LEAVES: usize = 32;
pub const MAX_CONDITION_LIST_ITEMS: usize = 100;
pub const MAX_ARGUMENT_BYTES: usize = 1024;
pub const MAX_PATH_BYTES: usize = 256;
pub const MAX_REGEX_SIZE_BYTES: usize = 1024 * 1024;
pub const MAX_RESPOND_HEADERS: usize = 20;
pub const MAX_RESPOND_BODY_BYTES: usize = 64 * 1024;
pub const MAX_DELAY_MS: i64 = 10_000;
pub const MAX_HTTP_TIMEOUT_MS: i64 = 15_000;
pub const DEFAULT_HTTP_TIMEOUT_MS: i64 = 10_000;
pub const MAX_HTTP_HEADERS: usize = 20;
pub const MAX_METHOD_BYTES: usize = 16;
pub const MAX_TARGET_URL_BYTES: usize = 2048;
pub const MAX_ALLOWLIST_ENTRIES: usize = 50;
pub const MAX_EXTRACT_GROUP: i64 = 99;
pub const MAX_INCREMENT_BY: i64 = 1_000_000_000;
pub const MAX_S3_PREFIX_BYTES: usize = 512;
pub const MAX_S3_REGION_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepScope {
    Every,
    Only(usize),
    Nothing,
}

impl StepScope {
    pub fn covers(self, index: usize) -> bool {
        match self {
            Self::Every => true,
            Self::Only(only) => only == index,
            Self::Nothing => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleLimits {
    pub max_rules: usize,
    pub max_steps: usize,
    pub steps: StepScope,
}

impl RuleLimits {
    pub const UNBOUNDED: Self = Self {
        max_rules: usize::MAX,
        max_steps: usize::MAX,
        steps: StepScope::Nothing,
    };

    pub fn with_step_cap(self, max_steps: usize) -> Self {
        Self { max_steps, ..self }
    }
}

impl Default for RuleLimits {
    fn default() -> Self {
        Self {
            max_rules: MAX_RULES_PER_ENDPOINT,
            max_steps: MAX_STEPS_PER_RULE,
            steps: StepScope::Every,
        }
    }
}
