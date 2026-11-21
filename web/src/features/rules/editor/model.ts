import type { Rule, RuleWriteBody } from '../../../api/rulesTypes';
import type { FieldError } from '../../../api/types';
import { TRIGGER_KINDS } from '../conditions/constants';
import { mapFieldErrors } from '../conditions/fieldErrors';
import { newGroup, parseCondition, serializeConditions, walkNodes, type ParseResult } from '../conditions/model';
import type { ConditionsDraft, IssueMap, RowIssue } from '../conditions/types';
import { validateConditions } from '../conditions/validation';
import { createStep, defaultFieldValue, findEntry, isEmptyValue, nextStepId, validateStep } from '../steps/model';
import type {
  FieldValue,
  KeyValuePair,
  RuleStep as StepDraft,
  StepCatalogueEntry,
  StepFieldSchema,
} from '../steps/types';
import type {
  FocusTarget,
  RuleCatalogue,
  RuleDraft,
  RuleValidation,
  ServerProblems,
  ServerStepError,
  StepProblem,
} from './types';

export const NAME_MAX_LENGTH = 100;

export interface DraftWire {
  input: RuleWriteBody;
  paths: ReturnType<typeof serializeConditions>['paths'];
}

type UnknownRecord = Record<string, unknown>;

const COMMON_KEYS: readonly string[] = ['kind', 'name', 'continue_on_error'];

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function fail(reason: string): ParseResult<never> {
  return { ok: false, reason };
}

function ownsName(entry: StepCatalogueEntry): boolean {
  return entry.fields.some((field) => field.name === 'name');
}

function pairsOf(raw: unknown): KeyValuePair[] | undefined {
  if (!Array.isArray(raw)) {
    return undefined;
  }
  const pairs: KeyValuePair[] = [];
  for (const item of raw) {
    if (!isRecord(item) || typeof item.name !== 'string' || typeof item.value !== 'string') {
      return undefined;
    }
    pairs.push({ name: item.name, value: item.value });
  }
  return pairs;
}

function fieldValueOf(field: StepFieldSchema, raw: unknown): FieldValue | undefined {
  if (raw === null) {
    return field.nullable === true ? null : defaultFieldValue(field);
  }
  switch (field.kind) {
    case 'boolean':
      return typeof raw === 'boolean' ? raw : undefined;
    case 'number':
    case 'duration':
      return typeof raw === 'number' && Number.isFinite(raw) ? raw : undefined;
    case 'key_value':
      return pairsOf(raw);
    default:
      return typeof raw === 'string' ? raw : undefined;
  }
}

function parseStep(value: unknown, catalogue: RuleCatalogue): ParseResult<StepDraft> {
  if (!isRecord(value) || typeof value.kind !== 'string') {
    return fail('step');
  }
  const entry = findEntry(catalogue.steps, value.kind);
  if (entry === undefined) {
    return fail(`kind ${value.kind}`);
  }
  const named = !ownsName(entry);
  let name: string | null = null;
  if (named && value.name !== undefined && value.name !== null) {
    if (typeof value.name !== 'string') {
      return fail('name');
    }
    name = value.name;
  }
  const flag = value.continue_on_error;
  if (flag !== undefined && flag !== null && typeof flag !== 'boolean') {
    return fail('continue_on_error');
  }
  const step = createStep(entry, nextStepId());
  step.name = name;
  step.continueOnError = flag === true;
  for (const key of Object.keys(value)) {
    if (COMMON_KEYS.includes(key) && (key !== 'name' || named)) {
      continue;
    }
    const field = entry.fields.find((candidate) => candidate.name === key);
    const parsed = field === undefined ? undefined : fieldValueOf(field, value[key]);
    if (field === undefined || parsed === undefined) {
      return fail(`field ${key}`);
    }
    step.fields[key] = parsed;
  }
  return { ok: true, value: step };
}

export function parseRuleInput(input: RuleWriteBody, catalogue: RuleCatalogue): ParseResult<RuleDraft> {
  if (input.name !== undefined && typeof input.name !== 'string') {
    return fail('name');
  }
  const root = parseCondition(input.condition ?? {}, catalogue.conditions);
  if (!root.ok) {
    return root;
  }
  const steps: StepDraft[] = [];
  for (const item of input.steps ?? []) {
    const parsed = parseStep(item, catalogue);
    if (!parsed.ok) {
      return parsed;
    }
    steps.push(parsed.value);
  }
  return { ok: true, value: { name: input.name ?? '', root: root.value, steps } };
}

export function inputOfRule(rule: Rule): RuleWriteBody {
  return { name: rule.name, condition: rule.condition, steps: rule.steps };
}

function wireValue(field: StepFieldSchema, value: FieldValue | undefined): unknown {
  if (value === undefined || value === null) {
    return undefined;
  }
  if (typeof value === 'string') {
    return value === '' && field.required !== true ? undefined : value;
  }
  if (Array.isArray(value)) {
    return value
      .filter((pair) => pair.name !== '' || pair.value !== '')
      .map((pair) => ({ name: pair.name, value: pair.value }));
  }
  return value;
}

function stepToWire(step: StepDraft, entry: StepCatalogueEntry): UnknownRecord {
  const out: UnknownRecord = { kind: step.kind };
  if (!ownsName(entry) && step.name !== null && step.name !== '') {
    out.name = step.name;
  }
  if (step.continueOnError) {
    out.continue_on_error = true;
  }
  for (const field of entry.fields) {
    const wire = wireValue(field, step.fields[field.name]);
    if (wire !== undefined) {
      out[field.name] = wire;
    }
  }
  return out;
}

export function stepsToWire(steps: readonly StepDraft[], catalogue: RuleCatalogue): UnknownRecord[] {
  return steps.map((step) => {
    const entry = findEntry(catalogue.steps, step.kind);
    return entry === undefined ? { kind: step.kind } : stepToWire(step, entry);
  });
}

function conditionsDraftOf(root: RuleDraft['root']): ConditionsDraft {
  return { triggers: [...TRIGGER_KINDS], root, onMatch: 'next' };
}

export function draftToWire(draft: RuleDraft, catalogue: RuleCatalogue): DraftWire {
  const conditions = serializeConditions(conditionsDraftOf(draft.root), catalogue.conditions);
  return {
    input: {
      name: draft.name.trim(),
      condition: conditions.input.condition,
      steps: stepsToWire(draft.steps, catalogue),
    },
    paths: conditions.paths,
  };
}

export function draftKey(draft: RuleDraft, catalogue: RuleCatalogue): string {
  return JSON.stringify(draftToWire(draft, catalogue).input);
}

export function inputKey(input: RuleWriteBody, catalogue: RuleCatalogue): string {
  const parsed = parseRuleInput(input, catalogue);
  return parsed.ok ? draftKey(parsed.value, catalogue) : JSON.stringify(input);
}

export function blankDraft(): RuleDraft {
  return { name: '', root: newGroup(), steps: [] };
}

export function isScriptRuleInput(input: RuleWriteBody): boolean {
  const steps = input.steps ?? [];
  const [only] = steps;
  return steps.length === 1 && isRecord(only) && only.kind === 'script';
}

function takenNamesFor(steps: readonly StepDraft[], step: StepDraft): string[] {
  return steps.filter((other) => other.id !== step.id).flatMap((other) => (other.name === null ? [] : [other.name]));
}

function stepProblems(draft: RuleDraft, catalogue: RuleCatalogue): StepProblem[] {
  const problems: StepProblem[] = [];
  for (const step of draft.steps) {
    const entry = findEntry(catalogue.steps, step.kind);
    for (const field of entry?.fields ?? []) {
      if (field.required === true && isEmptyValue(step.fields[field.name])) {
        problems.push({ stepId: step.id, field: field.name, kind: 'required', shape: null });
      }
    }
    if (entry !== undefined && ownsName(entry) && step.name !== null && step.name !== '') {
      problems.push({ stepId: step.id, field: null, kind: 'unsupportedName', shape: null });
    }
    for (const found of validateStep(entry, step, takenNamesFor(draft.steps, step))) {
      problems.push({ stepId: step.id, field: found.field, kind: 'shape', shape: found.issue });
    }
  }
  return problems;
}

export function validateRule(draft: RuleDraft, catalogue: RuleCatalogue): RuleValidation {
  const trimmed = draft.name.trim();
  const conditions = validateConditions(conditionsDraftOf(draft.root), catalogue.conditions);
  const steps = stepProblems(draft, catalogue);
  const nameMissing = trimmed === '';
  const nameTooLong = trimmed.length > NAME_MAX_LENGTH;
  const stepsMissing = draft.steps.length === 0;
  const count =
    (nameMissing ? 1 : 0) + (nameTooLong ? 1 : 0) + (stepsMissing ? 1 : 0) + conditions.count + steps.length;
  return { nameMissing, nameTooLong, stepsMissing, conditions, steps, count };
}

export function firstProblem(draft: RuleDraft, validation: RuleValidation): FocusTarget | null {
  if (validation.nameMissing || validation.nameTooLong) {
    return { kind: 'name' };
  }
  for (const node of walkNodes(draft.root)) {
    const first = validation.conditions.rows.get(node.id)?.[0];
    if (first !== undefined) {
      return { kind: 'row', id: node.id, part: first.part };
    }
  }
  const step = validation.steps[0];
  if (step !== undefined) {
    return { kind: 'step', stepId: step.stepId, field: step.field };
  }
  return validation.stepsMissing ? { kind: 'steps' } : null;
}

const STEP_PATH = /^steps\.(\d+)(?:\.([^.]+))?/;

function stepErrorOf(draft: RuleDraft, catalogue: RuleCatalogue, error: FieldError): ServerStepError | null {
  const match = STEP_PATH.exec(error.field);
  if (match === null) {
    return null;
  }
  const step = draft.steps[Number(match[1])];
  if (step === undefined) {
    return null;
  }
  const head = match[2] ?? null;
  const entry = findEntry(catalogue.steps, step.kind);
  const known = head !== null && (entry?.fields.some((field) => field.name === head) ?? false);
  return { stepId: step.id, field: known ? head : null, message: error.message };
}

function startsAt(field: string, path: string): boolean {
  return field === path || field.startsWith(`${path}.`);
}

export function mapServerErrors(
  details: readonly FieldError[],
  wire: DraftWire,
  draft: RuleDraft,
  catalogue: RuleCatalogue,
): ServerProblems {
  const conditionErrors = details.filter((error) => startsAt(error.field, 'condition'));
  const mapped = mapFieldErrors(conditionErrors, wire.paths, draft.root, catalogue.conditions);
  const name: FieldError[] = [];
  const steps: ServerStepError[] = [];
  const general: FieldError[] = [...mapped.general];
  for (const error of details) {
    if (startsAt(error.field, 'condition')) {
      continue;
    }
    if (startsAt(error.field, 'name')) {
      name.push(error);
      continue;
    }
    const step = stepErrorOf(draft, catalogue, error);
    if (step === null) {
      general.push(error);
    } else {
      steps.push(step);
    }
  }
  return { rows: mapped.rows, name, steps, general };
}

export function firstServerTarget(problems: ServerProblems, draft: RuleDraft): FocusTarget | null {
  if (problems.name.length > 0) {
    return { kind: 'name' };
  }
  for (const node of walkNodes(draft.root)) {
    const first = problems.rows.get(node.id)?.[0];
    if (first !== undefined) {
      return { kind: 'row', id: node.id, part: first.part };
    }
  }
  const step = problems.steps[0];
  return step === undefined ? null : { kind: 'step', stepId: step.stepId, field: step.field };
}

export function phaseSplit(steps: readonly StepDraft[], catalogue: RuleCatalogue): { before: number; after: number } {
  let lastResponse = -1;
  steps.forEach((step, index) => {
    if (findEntry(catalogue.steps, step.kind)?.class === 'response') {
      lastResponse = index;
    }
  });
  return { before: lastResponse + 1, after: steps.length - (lastResponse + 1) };
}

export function mergeIssues(client: IssueMap, server: IssueMap): IssueMap {
  const merged = new Map<string, RowIssue[]>();
  for (const [id, list] of [...client, ...server]) {
    merged.set(id, [...(merged.get(id) ?? []), ...list]);
  }
  return merged;
}
