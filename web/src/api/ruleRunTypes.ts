export type RuleRunState = 'sync' | 'queued' | 'running' | 'succeeded' | 'failed';
export type RuleRunTrigger = 'capture' | 'rerun' | 'dry_run';
export type RuleStepOutcome = 'ok' | 'failed' | 'skipped' | 'deferred' | 'no_op';

export interface RuleRunRuleEntry {
  rule_id: string;
  rule_name: string;
  matched: boolean;
}

export interface RuleRunStepEntry {
  rule_id: string;
  step: number;
  name: string | null;
  kind: string;
  outcome: RuleStepOutcome;
  duration_ms: number;
  detail: string | null;
  output: unknown;
}

export type RuleRunEntry = RuleRunRuleEntry | RuleRunStepEntry;

export interface RuleRun {
  id: string;
  capture_id: string;
  trigger: RuleRunTrigger;
  state: RuleRunState;
  rules_version: number;
  response_status: number | null;
  error: string | null;
  entries: RuleRunEntry[];
  created_at: string;
  finished_at: string | null;
}

export interface RuleRunPage {
  items: RuleRun[];
  next_cursor: string | null;
}

export interface RuleRunFilter {
  state?: RuleRunState;
}

export interface RuleResultPayload {
  capture_id: string;
  run: RuleRun;
}
