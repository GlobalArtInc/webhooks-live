import type { Capture } from '../../../api/types';
import type { DraftRuleInput, JsonObject, JsonValue } from './types';

export interface SimulationOptions {
  extended: boolean;
}

interface Collected {
  list: JsonObject[];
}

interface RunState {
  vars: JsonObject;
  stopped: boolean;
  failure: string | null;
  status: number | null;
  response: JsonObject | null;
}

const TEMPLATE = /\{\{\s*([a-z0-9_.-]+)\s*\}\}/gi;
const FAILURE_MARKER = 'boom';
const DEFERRED_KINDS: readonly string[] = ['http', 'forward', 'store_s3', 'notify'];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function textOf(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function bodyJson(capture: Capture): unknown {
  if (capture.body.encoding !== 'utf8') {
    return null;
  }
  try {
    return JSON.parse(capture.body.data);
  } catch (failure: unknown) {
    if (failure instanceof SyntaxError) {
      return null;
    }
    throw failure;
  }
}

function lookup(value: unknown, path: readonly string[]): unknown {
  let current: unknown = value;
  for (const key of path) {
    if (!isRecord(current)) {
      return undefined;
    }
    current = current[key];
  }
  return current;
}

export function subjectValue(capture: Capture, vars: JsonObject, subject: string): string | null {
  const parts = subject.split('.');
  if (subject === 'request.method') {
    return capture.method;
  }
  if (subject === 'request.path') {
    return capture.path;
  }
  if (parts[0] === 'request' && parts[1] === 'header') {
    const name = parts.slice(2).join('.').toLowerCase();
    return capture.headers.find((pair) => pair.name.toLowerCase() === name)?.value ?? null;
  }
  if (parts[0] === 'request' && parts[1] === 'query') {
    const name = parts.slice(2).join('.');
    return capture.query.find((pair) => pair.name === name)?.value ?? null;
  }
  if (parts[0] === 'request' && parts[1] === 'json') {
    const found = lookup(bodyJson(capture), parts.slice(2));
    return found === undefined || found === null ? null : String(found);
  }
  if (parts[0] === 'var' && parts[1] !== undefined) {
    const found = vars[parts[1]];
    return found === undefined || found === null ? null : String(found);
  }
  return null;
}

function render(template: string, capture: Capture, vars: JsonObject): string {
  return template.replace(TEMPLATE, (_match, path: string) => subjectValue(capture, vars, path) ?? '');
}

function compare(op: string, actual: string | null, expected: string | null): boolean {
  if (op === 'exists') {
    return actual !== null;
  }
  if (actual === null || expected === null) {
    return false;
  }
  switch (op) {
    case 'eq':
      return actual === expected;
    case 'ne':
      return actual !== expected;
    case 'contains':
      return actual.includes(expected);
    case 'starts_with':
      return actual.startsWith(expected);
    case 'ends_with':
      return actual.endsWith(expected);
    case 'gt':
      return Number(actual) > Number(expected);
    case 'lt':
      return Number(actual) < Number(expected);
    default:
      return false;
  }
}

function evaluate(condition: unknown, capture: Capture, vars: JsonObject, collected: Collected): boolean {
  if (!isRecord(condition)) {
    return true;
  }
  if (Array.isArray(condition.all)) {
    return condition.all.every((child: unknown) => evaluate(child, capture, vars, collected));
  }
  if (Array.isArray(condition.any)) {
    return condition.any.some((child: unknown) => evaluate(child, capture, vars, collected));
  }
  if ('not' in condition) {
    return !evaluate(condition.not, capture, vars, collected);
  }
  if (typeof condition.value !== 'string') {
    return true;
  }
  const actual = subjectValue(capture, vars, condition.value);
  const expected = condition.arg === undefined || condition.arg === null ? null : String(condition.arg);
  const result = compare(textOf(condition.op), actual, expected);
  collected.list.push({ subject: condition.value, op: textOf(condition.op), expected, actual, result });
  return result;
}

interface StepOutcome {
  outcome: string;
  detail: string | null;
  output: JsonValue | null;
}

function runStep(step: Record<string, unknown>, capture: Capture, state: RunState): StepOutcome {
  const kind = textOf(step.kind);
  if (DEFERRED_KINDS.includes(kind)) {
    return { outcome: 'deferred', detail: 'runs after the response', output: null };
  }
  if (kind === 'set_store') {
    const name = textOf(step.name);
    return { outcome: 'no_op', detail: `would set store.${name}`, output: { name, value: textOf(step.value) } };
  }
  if (kind === 'increment') {
    const counter = textOf(step.counter);
    return { outcome: 'no_op', detail: `would add 1 to counter.${counter}`, output: { counter, value: 1 } };
  }
  if (kind === 'respond') {
    const status = Number(render(textOf(step.status), capture, state.vars));
    const body = render(textOf(step.body), capture, state.vars);
    const headers = Array.isArray(step.headers) ? step.headers.length : 0;
    state.status = Number.isFinite(status) ? status : 200;
    state.response = {
      status: state.status,
      content_type: textOf(step.content_type),
      headers: [],
      body: { encoding: 'utf8', data: body, truncated: false },
      body_size: body.length,
    };
    return {
      outcome: 'ok',
      detail: null,
      output: { status: state.status, content_type: textOf(step.content_type), headers, body_bytes: body.length },
    };
  }
  if (kind === 'extract' || kind === 'set_var') {
    const into = textOf(step.into);
    const value =
      kind === 'set_var'
        ? render(textOf(step.value), capture, state.vars)
        : subjectValue(capture, state.vars, `request.json.${textOf(step.json_path).replace(/^\$\.?/, '')}`);
    if (value === null) {
      return { outcome: 'ok', detail: 'no match', output: null };
    }
    state.vars[into] = value;
    return { outcome: 'ok', detail: null, output: { into, value } };
  }
  if (kind === 'script') {
    if (textOf(step.source).includes(FAILURE_MARKER)) {
      state.failure = 'script: runtime error at line 2, column 5: boom';
      return { outcome: 'failed', detail: state.failure, output: null };
    }
    return { outcome: 'ok', detail: null, output: { vars: [], response: false, operations: 3, log: '' } };
  }
  if (kind === 'stop') {
    state.stopped = true;
    return { outcome: 'ok', detail: 'later steps and rules are skipped', output: null };
  }
  return { outcome: 'ok', detail: null, output: {} };
}

function runRule(
  rule: DraftRuleInput,
  index: number,
  capture: Capture,
  state: RunState,
  entries: JsonObject[],
  options: SimulationOptions,
): void {
  const ruleId = textOf(rule.id) || `draft-${index}`;
  const collected: Collected = { list: [] };
  const matched = evaluate(rule.condition, capture, state.vars, collected);
  const entry: JsonObject = { rule_id: ruleId, rule_name: textOf(rule.name), matched };
  if (options.extended) {
    entry.conditions = collected.list;
  }
  entries.push(entry);
  if (!matched) {
    return;
  }
  const steps = Array.isArray(rule.steps) ? rule.steps.filter(isRecord) : [];
  for (const [position, step] of steps.entries()) {
    const done = runStep(step, capture, state);
    const stepEntry: JsonObject = {
      rule_id: ruleId,
      step: position,
      name: textOf(step.name) || null,
      kind: textOf(step.kind),
      outcome: done.outcome,
      duration_ms: position + 1,
      detail: done.detail,
      output: done.output,
    };
    if (options.extended) {
      stepEntry.vars = { ...state.vars };
    }
    entries.push(stepEntry);
    if (state.failure !== null || state.stopped) {
      return;
    }
  }
  if (rule.on_match === 'stop') {
    state.stopped = true;
  }
}

export function simulateDryRun(
  rules: readonly DraftRuleInput[],
  capture: Capture,
  options: SimulationOptions,
): JsonObject {
  const state: RunState = { vars: {}, stopped: false, failure: null, status: null, response: null };
  const entries: JsonObject[] = [];
  for (const [index, rule] of rules.entries()) {
    runRule(rule, index, capture, state, entries, options);
    if (state.failure !== null || state.stopped) {
      break;
    }
  }
  const failed = state.failure !== null;
  const now = new Date().toISOString();
  const run: JsonObject = {
    id: capture.id,
    capture_id: capture.id,
    trigger: 'dry_run',
    state: failed ? 'failed' : 'succeeded',
    rules_version: 1,
    response_status: failed ? null : state.status,
    error: state.failure,
    entries,
    created_at: now,
    finished_at: now,
  };
  if (options.extended && state.response !== null && !failed) {
    run.response = state.response;
  }
  return run;
}
