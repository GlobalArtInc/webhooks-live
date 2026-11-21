import type { Capture } from '../../../api/types';
import { FLATTEN_DEPTH_LIMIT, FLATTEN_ENTRY_LIMIT, JSON_BODY_LIMIT_CHARS, MAX_VARIABLES } from './constants';
import { boundJson, clip } from './parse';
import type { JsonObject, JsonValue, TraceItem, TraceModel, VariableGroup, VariableRow } from './types';

interface Slot {
  group: VariableGroup;
  text: string;
  known: boolean;
}

type Snapshot = Map<string, Slot>;

const UNKNOWN_TEXT = '';

export function valueText(value: JsonValue): string {
  return clip(JSON.stringify(value) ?? 'null', 2048);
}

function isObject(value: JsonValue | undefined): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function flatten(value: JsonValue, path: string, depth: number, out: Snapshot): void {
  if (out.size >= FLATTEN_ENTRY_LIMIT) {
    return;
  }
  if (depth < FLATTEN_DEPTH_LIMIT && isObject(value)) {
    for (const [key, item] of Object.entries(value)) {
      flatten(item, `${path}.${key}`, depth + 1, out);
    }
    return;
  }
  if (depth < FLATTEN_DEPTH_LIMIT && Array.isArray(value)) {
    value.forEach((item, index) => {
      flatten(item, `${path}.${index}`, depth + 1, out);
    });
    return;
  }
  out.set(path, { group: 'request', text: valueText(value), known: true });
}

function jsonBody(capture: Capture): JsonValue | null {
  const { body } = capture;
  if (body.encoding !== 'utf8' || body.data === '' || body.data.length > JSON_BODY_LIMIT_CHARS) {
    return null;
  }
  try {
    return boundJson(JSON.parse(body.data));
  } catch (failure: unknown) {
    if (failure instanceof SyntaxError) {
      return null;
    }
    throw failure;
  }
}

function setRequest(out: Snapshot, path: string, text: string): void {
  out.set(path, { group: 'request', text: valueText(clip(text, 512)), known: true });
}

export function requestSnapshot(capture: Capture | undefined): Snapshot {
  const out: Snapshot = new Map();
  if (capture === undefined) {
    return out;
  }
  setRequest(out, 'request.method', capture.method);
  setRequest(out, 'request.path', capture.path);
  for (const pair of capture.query) {
    setRequest(out, `request.query.${pair.name}`, pair.value);
  }
  for (const pair of capture.headers) {
    setRequest(out, `request.header.${pair.name.toLowerCase()}`, pair.value);
  }
  const parsed = jsonBody(capture);
  if (parsed !== null) {
    const json: Snapshot = new Map();
    flatten(parsed, 'request.json', 0, json);
    for (const [path, slot] of json) {
      out.set(path, slot);
    }
  }
  return out;
}

function record(value: JsonValue | null): JsonObject | null {
  return value !== null && isObject(value) ? value : null;
}

function put(out: Snapshot, group: VariableGroup, name: string, value: JsonValue | undefined): void {
  out.set(`${group}.${name}`, {
    group,
    text: value === undefined ? UNKNOWN_TEXT : valueText(value),
    known: value !== undefined,
  });
}

function applyOutput(out: Snapshot, item: TraceItem): void {
  const output = record(item.output);
  if (output === null) {
    return;
  }
  const into = output.into;
  if (
    (item.stepKind === 'extract' || item.stepKind === 'set_var' || item.stepKind === 'transform') &&
    typeof into === 'string'
  ) {
    if (into !== 'payload') {
      put(out, 'var', into, output.value);
    }
    return;
  }
  if (item.stepKind === 'set_store' && typeof output.name === 'string') {
    put(out, 'store', output.name, output.value);
    return;
  }
  if (item.stepKind === 'increment' && typeof output.counter === 'string') {
    put(out, 'counter', output.counter, output.value);
    return;
  }
  if (item.stepKind === 'script' && Array.isArray(output.vars)) {
    for (const name of output.vars) {
      if (typeof name === 'string' && !out.has(`var.${name}`)) {
        put(out, 'var', name, undefined);
      }
    }
  }
}

function applyItem(out: Snapshot, item: TraceItem): void {
  if (item.type !== 'step' || item.placeholder || (item.status !== 'ran' && item.status !== 'simulated')) {
    return;
  }
  applyOutput(out, item);
  if (item.vars === null) {
    return;
  }
  for (const key of [...out.keys()]) {
    if (key.startsWith('var.')) {
      out.delete(key);
    }
  }
  for (const [name, value] of Object.entries(item.vars)) {
    put(out, 'var', name, value);
  }
}

function snapshotAt(model: TraceModel, base: Snapshot, index: number): Snapshot {
  const out: Snapshot = new Map(base);
  for (let position = 0; position <= index && position < model.items.length; position += 1) {
    const item = model.items[position];
    if (item !== undefined) {
      applyItem(out, item);
    }
  }
  return out;
}

export function variableRowsAt(model: TraceModel, capture: Capture | undefined, index: number): VariableRow[] {
  const base = requestSnapshot(capture);
  const current = snapshotAt(model, base, index);
  const before = index <= 0 ? base : snapshotAt(model, base, index - 1);
  const rows: VariableRow[] = [];
  for (const [path, slot] of current) {
    const earlier = before.get(path);
    const change = index <= 0 || earlier?.text === slot.text ? 'same' : earlier === undefined ? 'new' : 'changed';
    rows.push({ path, text: slot.text, change, group: slot.group, known: slot.known });
  }
  return rows.slice(0, MAX_VARIABLES);
}

export function filterRows(rows: readonly VariableRow[], query: string): VariableRow[] {
  const needle = query.trim().toLowerCase();
  return needle === '' ? [...rows] : rows.filter((row) => row.path.toLowerCase().includes(needle));
}
