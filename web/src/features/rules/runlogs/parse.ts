import type {
  RuleResultPayload,
  RuleRun,
  RuleRunEntry,
  RuleRunState,
  RuleRunTrigger,
  RuleStepOutcome,
} from '../../../api/ruleRunTypes';

type UnknownRecord = Record<string, unknown>;

const RUN_STATES: readonly unknown[] = ['sync', 'queued', 'running', 'succeeded', 'failed'];
const RUN_TRIGGERS: readonly unknown[] = ['capture', 'rerun', 'dry_run'];
const STEP_OUTCOMES: readonly unknown[] = ['ok', 'failed', 'skipped', 'deferred', 'no_op'];

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isCount(value: unknown): value is number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === 'string';
}

function isNullableNumber(value: unknown): value is number | null {
  return value === null || typeof value === 'number';
}

export function isRuleRunState(value: unknown): value is RuleRunState {
  return RUN_STATES.includes(value);
}

function isTrigger(value: unknown): value is RuleRunTrigger {
  return RUN_TRIGGERS.includes(value);
}

function isOutcome(value: unknown): value is RuleStepOutcome {
  return STEP_OUTCOMES.includes(value);
}

function isEntry(value: unknown): value is RuleRunEntry {
  if (!isRecord(value) || typeof value.rule_id !== 'string') {
    return false;
  }
  if ('matched' in value) {
    return typeof value.rule_name === 'string' && typeof value.matched === 'boolean';
  }
  return (
    isCount(value.step) &&
    isNullableString(value.name) &&
    typeof value.kind === 'string' &&
    isOutcome(value.outcome) &&
    isCount(value.duration_ms) &&
    isNullableString(value.detail)
  );
}

export function isRuleRun(value: unknown): value is RuleRun {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.capture_id === 'string' &&
    isTrigger(value.trigger) &&
    isRuleRunState(value.state) &&
    isCount(value.rules_version) &&
    isNullableNumber(value.response_status) &&
    isNullableString(value.error) &&
    Array.isArray(value.entries) &&
    value.entries.every(isEntry) &&
    typeof value.created_at === 'string' &&
    isNullableString(value.finished_at)
  );
}

export function readRuleResult(value: unknown): RuleResultPayload | null {
  if (!isRecord(value) || typeof value.capture_id !== 'string' || !isRuleRun(value.run)) {
    return null;
  }
  return { capture_id: value.capture_id, run: value.run };
}
