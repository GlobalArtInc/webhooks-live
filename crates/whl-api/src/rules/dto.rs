use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use whl_store::rules::{RuleRecord, RuleSet};

pub(super) const GRAMMAR: &str = "`condition` (Condition, recursive): an object with one of: nothing (`{}`, always matches, the default); `all`: array of Conditions; `any`: array of Conditions; `not`: one Condition; or a leaf with `value` (a variable path such as `request.method` or `request.header.x-event-type`), `op` and `arg`. `op` is one of `eq`, `ne`, `contains`, `starts_with`, `ends_with`, `matches` (regex), `exists` (no `arg`), `gt`, `lt`, `in` (list), `in_cidr` (list). `arg` is a string, a number, an array of strings or null. At most 4 levels and 32 leaves. `steps` (Step[], between 1 and the tier's steps per rule): each step has `kind`, optional `name` (`^[a-z][a-z0-9_]{0,31}$`, unique per endpoint, required when a later step reads its result), `continue_on_error` (default false) and the fields of its kind: `respond`: status (template), content_type (template or null), headers (name and value pairs, values templated, at most 20), body (template, at most 64 KiB); `set_header`: name, value (template); `remove_header`: name; `delay`: ms (0 to 10000); `stop`: none; `extract`: from (variable path), json_path or regex (with group, default 1), into (run variable); `set_var`: into, value (template); `transform`: into (run variable or `payload`), template; `set_store`: name (persistent variable), value (template); `increment`: counter (persistent variable), by (integer, default 1); `script`: source (Rhai, at most 16 KiB); `http`: method, url (template), headers (name and value pairs, templated), body (template or null), timeout_ms (at most 15000, default 10000), follow_redirects (default true); `forward`: forward_rule_id, or target_url with optional method, content_type, header_allowlist; body_from `capture` (default) or `payload`; `notify`: channel_id, message (template or null for the channel default); `store_s3`: bucket_url, prefix, region, access_key_secret, secret_key_secret, content `capture_json` or `body`. Templates are plain strings in the template syntax of architecture section 21.";

pub(super) const RUN_GRAMMAR: &str = "`trigger`: `capture`, `rerun` or `dry_run`. `state`: `sync`, `queued`, `running`, `succeeded` or `failed`. `error`: null when the run did not fail, else `template`, `budget`, `script`, `http`, `quota_exceeded`, `overloaded`, `hop_limit` or `internal`, then `: ` and a detail. `entries` (in execution order): a rule entry is `{rule_id, rule_name, matched}` (a dry run adds `error` when the condition of a rule after the response failed to evaluate: the rule is then reported as not matched); a step entry is `{rule_id, step (index), name, kind, outcome (ok, failed, skipped, deferred or no_op), duration_ms, detail, output}`. `output` holds at most 4 KiB: the HTTP result `{status, headers, body, duration_ms}`, extracted values or script output. Secrets appear as `[secret:NAME]`.";

#[derive(Serialize, ToSchema)]
#[schema(as = Rule, description = GRAMMAR)]
pub struct RuleDto {
    #[schema(
        read_only,
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    id: String,
    #[schema(read_only, example = 0)]
    position: i16,
    #[schema(min_length = 1, max_length = 100, example = "Refund mock")]
    name: String,
    #[schema(default = true, example = true)]
    enabled: bool,
    #[schema(value_type = Object)]
    condition: Value,
    #[schema(value_type = Vec<Object>, min_items = 1)]
    steps: Value,
    #[schema(read_only, format = DateTime)]
    created_at: String,
    #[schema(read_only, format = DateTime)]
    updated_at: String,
}

impl From<&RuleRecord> for RuleDto {
    fn from(rule: &RuleRecord) -> Self {
        Self {
            id: rule.id.hyphenated().to_string(),
            position: rule.position,
            name: rule.name.clone(),
            enabled: rule.enabled,
            condition: rule.condition.clone(),
            steps: rule.steps.clone(),
            created_at: rule.created_at.clone(),
            updated_at: rule.updated_at.clone(),
        }
    }
}

#[derive(Serialize, ToSchema)]
#[schema(
    as = RuleList,
    description = "The rules in execution order. `version` is bumped by every change to the endpoint's rules."
)]
pub struct RuleSetDto {
    items: Vec<RuleDto>,
    #[schema(example = 3)]
    version: i32,
}

impl From<&RuleSet> for RuleSetDto {
    fn from(set: &RuleSet) -> Self {
        Self {
            items: set.rules.iter().map(RuleDto::from).collect(),
            version: set.version,
        }
    }
}

#[derive(ToSchema)]
#[schema(as = RuleInput, description = GRAMMAR)]
#[allow(dead_code)]
pub struct RuleInputDoc {
    #[schema(min_length = 1, max_length = 100, example = "Refund mock")]
    name: Option<String>,
    #[schema(default = true)]
    enabled: Option<bool>,
    #[schema(value_type = Object, required = false)]
    condition: Option<Value>,
    #[schema(value_type = Vec<Object>, min_items = 1, required = false)]
    steps: Option<Vec<Value>>,
}

#[derive(ToSchema)]
#[schema(
    as = RuleDryRunInput,
    description = "`capture_id` is required. `rules`, when present and not null, are unsaved RuleInput drafts to test instead of the saved rules (at most 5). `execute_http` must be a boolean when present; the server validates it and does not act on it."
)]
#[allow(dead_code)]
pub struct RuleDryRunInputDoc {
    #[schema(
        pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$",
        example = "0193a6c2-7d1e-7c3a-8f55-2b9e4c1a6d70"
    )]
    capture_id: String,
    #[schema(max_items = 5)]
    rules: Option<Vec<RuleInputDoc>>,
    #[schema(default = false)]
    execute_http: Option<bool>,
}

#[derive(ToSchema)]
#[schema(
    as = RuleOrderInput,
    description = "`ids` lists every rule id of the endpoint exactly once, in the new order (lowercase hyphenated UUIDs)."
)]
#[allow(dead_code)]
pub struct RuleOrderInputDoc {
    #[schema(max_items = 1024)]
    ids: Vec<String>,
}
