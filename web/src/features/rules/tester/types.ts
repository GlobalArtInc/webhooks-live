import type { Body, Capture, CaptureSummary, NameValue, SendResult } from '../../../api/types';
import type { Rule } from '../../../api/rulesTypes';

export interface JsonObject {
  [key: string]: JsonValue;
}

export type JsonValue = string | number | boolean | null | JsonValue[] | JsonObject;

export type EntryOutcome = 'ok' | 'failed' | 'skipped' | 'deferred' | 'no_op';

export type RunState = 'sync' | 'queued' | 'running' | 'succeeded' | 'failed';

export interface ConditionResult {
  subject: string;
  op: string;
  expected: string | null;
  actual: string | null;
  result: boolean;
}

export interface RuleRunRuleEntry {
  type: 'rule';
  ruleId: string;
  ruleName: string;
  matched: boolean;
  conditions: ConditionResult[] | null;
}

export interface RuleRunStepEntry {
  type: 'step';
  ruleId: string;
  step: number;
  name: string | null;
  kind: string;
  outcome: EntryOutcome;
  durationMs: number;
  detail: string | null;
  output: JsonValue | null;
  vars: JsonObject | null;
}

export type RuleRunEntry = RuleRunRuleEntry | RuleRunStepEntry;

export interface RuleRunResponse {
  status: number;
  contentType: string;
  headers: NameValue[];
  body: Body;
  bodySize: number;
}

export interface RuleRunResult {
  id: string;
  captureId: string;
  state: RunState;
  rulesVersion: number;
  responseStatus: number | null;
  error: string | null;
  entries: RuleRunEntry[];
  entriesTruncated: boolean;
  response: RuleRunResponse | null;
  createdAt: string;
  finishedAt: string | null;
}

export interface DraftRuleInput {
  [key: string]: unknown;
}

export interface DryRunRequest {
  capture_id: string;
  rules: DraftRuleInput[];
  execute_http: boolean;
}

export type TesterScope = 'rule' | 'all';

export interface TesterDraft {
  ruleId: string | null;
  input: DraftRuleInput;
}

export interface RealRunResult {
  send: SendResult;
  target: string;
}

export interface TesterTransport {
  listCaptures: (endpointId: string) => Promise<CaptureSummary[]>;
  getCapture: (endpointId: string, captureId: string) => Promise<Capture>;
  dryRun: (endpointId: string, request: DryRunRequest, signal: AbortSignal) => Promise<RuleRunResult>;
  sendReal: (endpointId: string, endpointUrl: string, capture: Capture) => Promise<RealRunResult>;
  listRules: (endpointId: string) => Promise<Rule[]>;
  sendSample: (endpointId: string, endpointUrl: string) => Promise<string | null>;
}

export type TraceStatus =
  'matched' | 'notMatched' | 'ran' | 'failed' | 'simulated' | 'skipped' | 'deferred' | 'notReached';

export interface TraceItem {
  id: string;
  type: 'rule' | 'step';
  group: number;
  ruleId: string | null;
  ruleName: string;
  stepIndex: number | null;
  position: number | null;
  stepKind: string | null;
  stepName: string | null;
  status: TraceStatus;
  durationMs: number;
  detail: string | null;
  output: JsonValue | null;
  conditions: ConditionResult[] | null;
  vars: JsonObject | null;
  placeholder: boolean;
}

export type CutReason = 'stop' | 'failed' | 'unknown';

export interface TraceGroup {
  index: number;
  ruleId: string | null;
  ruleName: string;
  matched: boolean | null;
  itemIds: string[];
  stepCount: number;
  ranCount: number;
  failedPosition: number | null;
  cut: CutReason | null;
}

export interface FinalResponse {
  status: number | null;
  groupIndex: number | null;
  contentType: string | null;
  headerCount: number | null;
  bodyBytes: number | null;
  detail: RuleRunResponse | null;
}

export interface TraceModel {
  items: TraceItem[];
  groups: TraceGroup[];
  final: FinalResponse;
  error: string | null;
  failed: boolean;
  truncated: boolean;
  matchedCount: number;
  simulatedCount: number;
  stepCount: number;
  durationMs: number;
}

export interface TraceSelection {
  itemId: string;
  ruleId: string | null;
  stepIndex: number | null;
}

export interface ErrorPosition {
  line: number;
  column: number;
}

export interface Cursor {
  resultId: number;
  selected: number;
  revealedTo: number | null;
}

export interface TesterRule {
  name: string;
  stepKinds: string[];
  stepNames: (string | null)[];
  stopsOnMatch: boolean;
}

export type VariableChange = 'new' | 'changed' | 'same';

export type VariableGroup = 'request' | 'var' | 'store' | 'counter';

export interface VariableRow {
  path: string;
  text: string;
  change: VariableChange;
  group: VariableGroup;
  known: boolean;
}

export interface RunInput {
  request: DryRunRequest;
  described: TesterRule[];
  draftIndex: number | null;
}

export interface RunOutcome {
  run: RuleRunResult;
  described: TesterRule[];
  draftIndex: number | null;
  captureId: string;
}
