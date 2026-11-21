import { createKeyValueRow, type KeyValueRow } from '../../../components/KeyValueEditor/rows';
import type {
  FieldValue,
  GuardArgument,
  GuardOperator,
  GuardPredicate,
  KeyValuePair,
  RuleStep,
  ShapeIssue,
  StepCatalogueEntry,
  StepFieldSchema,
  StepIssue,
} from './types';

export const GUARD_OPERATORS: readonly GuardOperator[] = [
  'eq',
  'ne',
  'contains',
  'starts_with',
  'ends_with',
  'matches',
  'exists',
  'gt',
  'lt',
  'in',
  'in_cidr',
];

const HEADER_TOKEN = /^[A-Za-z0-9!#$%&'*+.^_`|~-]+$/;
const STEP_NAME = /^[a-z][a-z0-9_]{0,31}$/;
const PLACEHOLDER = /\{(@name|[a-z_][a-z0-9_]*)\}/g;
const EMPTY_MARK = '…';

let stepCounter = 0;

export function nextStepId(): string {
  stepCounter += 1;
  return `step-${stepCounter}`;
}

export function defaultFieldValue(field: StepFieldSchema): FieldValue {
  if (field.defaultValue !== undefined) {
    return Array.isArray(field.defaultValue) ? field.defaultValue.map((pair) => ({ ...pair })) : field.defaultValue;
  }
  switch (field.kind) {
    case 'boolean':
      return false;
    case 'key_value':
      return [];
    case 'number':
    case 'duration':
      return null;
    case 'select':
      return field.required === true && field.options !== undefined ? (field.options[0]?.value ?? '') : '';
    default:
      return field.nullable === true ? null : '';
  }
}

export function createStep(entry: StepCatalogueEntry, id: string = nextStepId()): RuleStep {
  const fields: Record<string, FieldValue> = {};
  for (const field of entry.fields) {
    fields[field.name] = defaultFieldValue(field);
  }
  return { id, kind: entry.kind, name: null, continueOnError: false, guard: null, fields };
}

export function insertStep(steps: readonly RuleStep[], index: number, step: RuleStep): RuleStep[] {
  const at = Math.max(0, Math.min(index, steps.length));
  return [...steps.slice(0, at), step, ...steps.slice(at)];
}

export function removeStep(steps: readonly RuleStep[], index: number): RuleStep[] {
  return steps.filter((_, position) => position !== index);
}

export function moveStep(steps: readonly RuleStep[], index: number, delta: number): RuleStep[] {
  const target = index + delta;
  if (index < 0 || index >= steps.length || target < 0 || target >= steps.length) {
    return [...steps];
  }
  const next = [...steps];
  const [moved] = next.splice(index, 1);
  if (moved !== undefined) {
    next.splice(target, 0, moved);
  }
  return next;
}

export function replaceStep(steps: readonly RuleStep[], index: number, step: RuleStep): RuleStep[] {
  return steps.map((current, position) => (position === index ? step : current));
}

export function setStepField(step: RuleStep, name: string, value: FieldValue): RuleStep {
  return { ...step, fields: { ...step.fields, [name]: value } };
}

export function findEntry(catalogue: readonly StepCatalogueEntry[], kind: string): StepCatalogueEntry | undefined {
  return catalogue.find((entry) => entry.kind === kind);
}

export function isEmptyValue(value: FieldValue | undefined): boolean {
  if (value === undefined || value === null) {
    return true;
  }
  if (typeof value === 'string') {
    return value.trim() === '';
  }
  if (Array.isArray(value)) {
    return value.length === 0;
  }
  return false;
}

function valueText(value: FieldValue | undefined): string {
  if (value === undefined || value === null) {
    return EMPTY_MARK;
  }
  if (Array.isArray(value)) {
    return value.length === 0 ? EMPTY_MARK : String(value.length);
  }
  if (typeof value === 'string') {
    return value.trim() === '' ? EMPTY_MARK : value;
  }
  return String(value);
}

function fillPlaceholders(template: string, step: RuleStep, onEmpty: 'mark' | 'fail'): string | null {
  let failed = false;
  const filled = template.replace(PLACEHOLDER, (_, key: string) => {
    const raw = key === '@name' ? step.name : step.fields[key];
    if (onEmpty === 'fail' && isEmptyValue(raw)) {
      failed = true;
    }
    return valueText(raw);
  });
  return failed ? null : filled;
}

export function renderSummary(entry: StepCatalogueEntry, step: RuleStep): string | null {
  if (entry.summary === null) {
    return null;
  }
  return fillPlaceholders(entry.summary, step, 'mark');
}

export function producedVariables(entry: StepCatalogueEntry, step: RuleStep): string[] {
  const produced: string[] = [];
  for (const template of entry.produces) {
    const filled = fillPlaceholders(template, step, 'fail');
    if (filled !== null && !produced.includes(filled)) {
      produced.push(filled);
    }
  }
  return produced;
}

export function operatorNeedsArgument(op: GuardOperator): boolean {
  return op !== 'exists';
}

export function guardArgumentToText(arg: GuardArgument): string {
  if (arg === null) {
    return '';
  }
  if (Array.isArray(arg)) {
    return arg.join(', ');
  }
  return String(arg);
}

export function guardArgumentFromText(op: GuardOperator, text: string): GuardArgument {
  if (!operatorNeedsArgument(op)) {
    return null;
  }
  if (op === 'in' || op === 'in_cidr') {
    return text
      .split(',')
      .map((part) => part.trim())
      .filter((part) => part !== '');
  }
  if ((op === 'gt' || op === 'lt') && text.trim() !== '' && Number.isFinite(Number(text))) {
    return Number(text);
  }
  return text;
}

export function createGuard(): GuardPredicate {
  return { value: '', op: 'eq', arg: '' };
}

export function changeGuardOperator(guard: GuardPredicate, op: GuardOperator): GuardPredicate {
  return { value: guard.value, op, arg: guardArgumentFromText(op, guardArgumentToText(guard.arg)) };
}

export function isGuardComplete(guard: GuardPredicate): boolean {
  if (guard.value.trim() === '') {
    return false;
  }
  if (!operatorNeedsArgument(guard.op)) {
    return true;
  }
  const arg = guard.arg;
  if (arg === null) {
    return false;
  }
  if (Array.isArray(arg)) {
    return arg.length > 0;
  }
  return String(arg).trim() !== '';
}

const encoder = new TextEncoder();

export function fieldIssue(field: StepFieldSchema, value: FieldValue | undefined): ShapeIssue | null {
  if (isEmptyValue(value) || value === undefined || value === null) {
    return null;
  }
  if (typeof value === 'string') {
    if (field.maxLength !== undefined && value.length > field.maxLength) {
      return { code: 'tooLong', limit: field.maxLength };
    }
    if (field.maxBytes !== undefined && encoder.encode(value).length > field.maxBytes) {
      return { code: 'tooLarge', limit: field.maxBytes };
    }
    if (field.kind === 'header_name' && !HEADER_TOKEN.test(value)) {
      return { code: 'badHeaderName' };
    }
    if (field.kind === 'json_path' && !value.trim().startsWith('$')) {
      return { code: 'badJsonPath' };
    }
    return null;
  }
  if (typeof value === 'number') {
    if (!Number.isInteger(value)) {
      return { code: 'notInteger' };
    }
    if (field.min !== undefined && value < field.min) {
      return { code: 'tooSmall', limit: field.min };
    }
    if (field.max !== undefined && value > field.max) {
      return { code: 'tooBig', limit: field.max };
    }
    return null;
  }
  if (Array.isArray(value) && field.maxRows !== undefined && value.length > field.maxRows) {
    return { code: 'tooManyRows', limit: field.maxRows };
  }
  return null;
}

export function pairIssue(field: StepFieldSchema, pair: KeyValuePair): ShapeIssue | null {
  if (pair.name !== '' && field.nameKind === 'header_name' && !HEADER_TOKEN.test(pair.name)) {
    return { code: 'badHeaderName' };
  }
  return null;
}

export function validateStep(
  entry: StepCatalogueEntry | undefined,
  step: RuleStep,
  takenNames: readonly string[],
): StepIssue[] {
  const issues: StepIssue[] = [];
  for (const field of entry?.fields ?? []) {
    const issue = fieldIssue(field, step.fields[field.name]);
    if (issue !== null) {
      issues.push({ field: field.name, issue });
    }
  }
  if (step.name !== null && step.name !== '') {
    if (!STEP_NAME.test(step.name)) {
      issues.push({ field: 'name', issue: { code: 'badName' } });
    } else if (takenNames.includes(step.name)) {
      issues.push({ field: 'name', issue: { code: 'duplicateName' } });
    }
  }
  if (step.guard !== null && !isGuardComplete(step.guard)) {
    issues.push({ field: 'guard', issue: { code: 'guardIncomplete' } });
  }
  return issues;
}

export function rowsFromPairs(pairs: readonly KeyValuePair[]): KeyValueRow[] {
  return pairs.map((pair) => createKeyValueRow(pair.name, pair.value));
}

export function pairsFromRows(rows: readonly KeyValueRow[]): KeyValuePair[] {
  return rows.map((row) => ({ name: row.name, value: row.value }));
}

export function samePairs(rows: readonly KeyValueRow[], pairs: readonly KeyValuePair[]): boolean {
  return (
    rows.length === pairs.length &&
    rows.every((row, index) => row.name === pairs[index]?.name && row.value === pairs[index]?.value)
  );
}

export function formatDuration(ms: number, locale: string): string {
  if (ms < 1000) {
    return new Intl.NumberFormat(locale, { style: 'unit', unit: 'millisecond', unitDisplay: 'short' }).format(ms);
  }
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: 'second',
    unitDisplay: 'short',
    maximumFractionDigits: 2,
  }).format(ms / 1000);
}
