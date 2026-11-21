import type { Body, NameValue } from '../../../api/types';
import {
  JSON_DEPTH_LIMIT,
  JSON_ITEMS_LIMIT,
  JSON_NODE_LIMIT,
  MAX_CONDITIONS,
  MAX_ENTRIES,
  MAX_HEADERS,
  TEXT_LIMIT_CHARS,
} from './constants';
import type {
  ConditionResult,
  EntryOutcome,
  JsonObject,
  JsonValue,
  RuleRunEntry,
  RuleRunResponse,
  RuleRunResult,
  RunState,
} from './types';

type UnknownRecord = Record<string, unknown>;

const OUTCOMES: readonly EntryOutcome[] = ['ok', 'failed', 'skipped', 'deferred', 'no_op'];
const STATES: readonly RunState[] = ['sync', 'queued', 'running', 'succeeded', 'failed'];
const TRUNCATED_MARK = '...';

export class UnreadableResultError extends Error {
  constructor() {
    super('The test result is not a rule run');
    this.name = 'UnreadableResultError';
  }
}

interface NodeBudget {
  left: number;
}

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function clip(text: string, limit: number = TEXT_LIMIT_CHARS): string {
  return text.length <= limit ? text : `${text.slice(0, limit)}${TRUNCATED_MARK}`;
}

function textOf(value: unknown, fallback: string): string {
  return typeof value === 'string' ? clip(value) : fallback;
}

function nullableText(value: unknown): string | null {
  return typeof value === 'string' ? clip(value) : null;
}

function integerOf(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.trunc(value) : fallback;
}

function boundNode(value: unknown, depth: number, budget: NodeBudget): JsonValue {
  budget.left -= 1;
  if (budget.left < 0 || depth > JSON_DEPTH_LIMIT) {
    return TRUNCATED_MARK;
  }
  if (typeof value === 'string') {
    return clip(value);
  }
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : null;
  }
  if (typeof value === 'boolean' || value === null) {
    return value;
  }
  if (Array.isArray(value)) {
    const items: unknown[] = value;
    return items.slice(0, JSON_ITEMS_LIMIT).map((item) => boundNode(item, depth + 1, budget));
  }
  if (isRecord(value)) {
    const result: JsonObject = {};
    for (const [key, item] of Object.entries(value).slice(0, JSON_ITEMS_LIMIT)) {
      result[clip(key, 256)] = boundNode(item, depth + 1, budget);
    }
    return result;
  }
  return null;
}

export function boundJson(value: unknown): JsonValue {
  return boundNode(value, 0, { left: JSON_NODE_LIMIT });
}

function readOutput(record: UnknownRecord): JsonValue | null {
  return record.output === undefined || record.output === null ? null : boundJson(record.output);
}

function readVars(record: UnknownRecord): JsonObject | null {
  if (!isRecord(record.vars)) {
    return null;
  }
  const bounded = boundJson(record.vars);
  return typeof bounded === 'object' && bounded !== null && !Array.isArray(bounded) ? bounded : null;
}

function readCondition(value: unknown): ConditionResult | null {
  if (!isRecord(value) || typeof value.result !== 'boolean') {
    return null;
  }
  return {
    subject: textOf(value.subject, ''),
    op: textOf(value.op, ''),
    expected: nullableText(value.expected),
    actual: nullableText(value.actual),
    result: value.result,
  };
}

function isCondition(value: ConditionResult | null): value is ConditionResult {
  return value !== null;
}

function readConditions(record: UnknownRecord): ConditionResult[] | null {
  if (!Array.isArray(record.conditions)) {
    return null;
  }
  const items: unknown[] = record.conditions;
  return items.slice(0, MAX_CONDITIONS).map(readCondition).filter(isCondition);
}

function readOutcome(value: unknown): EntryOutcome | null {
  return OUTCOMES.find((outcome) => outcome === value) ?? null;
}

function readEntry(value: unknown): RuleRunEntry | null {
  if (!isRecord(value) || typeof value.rule_id !== 'string') {
    return null;
  }
  const ruleId = clip(value.rule_id, 128);
  if (typeof value.matched === 'boolean') {
    return {
      type: 'rule',
      ruleId,
      ruleName: textOf(value.rule_name, ''),
      matched: value.matched,
      conditions: readConditions(value),
    };
  }
  const outcome = readOutcome(value.outcome);
  if (outcome === null || typeof value.kind !== 'string') {
    return null;
  }
  return {
    type: 'step',
    ruleId,
    step: Math.max(0, integerOf(value.step, 0)),
    name: nullableText(value.name),
    kind: clip(value.kind, 64),
    outcome,
    durationMs: Math.max(0, integerOf(value.duration_ms, 0)),
    detail: nullableText(value.detail),
    output: readOutput(value),
    vars: readVars(value),
  };
}

function isEntry(value: RuleRunEntry | null): value is RuleRunEntry {
  return value !== null;
}

function readHeaders(value: unknown): NameValue[] {
  const items: unknown[] = Array.isArray(value) ? value : [];
  const headers: NameValue[] = [];
  for (const item of items.slice(0, MAX_HEADERS)) {
    if (isRecord(item) && typeof item.name === 'string' && typeof item.value === 'string') {
      headers.push({ name: clip(item.name, 256), value: clip(item.value, 2048) });
    }
  }
  return headers;
}

function readBody(value: unknown): Body {
  const record = isRecord(value) ? value : {};
  return {
    encoding: record.encoding === 'base64' ? 'base64' : 'utf8',
    data: typeof record.data === 'string' ? record.data : '',
    truncated: record.truncated === true,
  };
}

function readResponse(value: unknown): RuleRunResponse | null {
  if (!isRecord(value) || typeof value.status !== 'number') {
    return null;
  }
  const body = readBody(value.body);
  return {
    status: Math.trunc(value.status),
    contentType: textOf(value.content_type, ''),
    headers: readHeaders(value.headers),
    body,
    bodySize: Math.max(0, integerOf(value.body_size, body.data.length)),
  };
}

function readState(value: unknown): RunState {
  return STATES.find((state) => state === value) ?? 'sync';
}

export function parseRuleRun(payload: unknown): RuleRunResult {
  if (!isRecord(payload) || !Array.isArray(payload.entries)) {
    throw new UnreadableResultError();
  }
  const rawEntries: unknown[] = payload.entries;
  const entries = rawEntries.slice(0, MAX_ENTRIES).map(readEntry).filter(isEntry);
  return {
    id: textOf(payload.id, ''),
    captureId: textOf(payload.capture_id, ''),
    state: readState(payload.state),
    rulesVersion: integerOf(payload.rules_version, 0),
    responseStatus: typeof payload.response_status === 'number' ? Math.trunc(payload.response_status) : null,
    error: nullableText(payload.error),
    entries,
    entriesTruncated: rawEntries.length > MAX_ENTRIES,
    response: readResponse(payload.response),
    createdAt: textOf(payload.created_at, ''),
    finishedAt: nullableText(payload.finished_at),
  };
}
