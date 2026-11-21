import type { Rule } from '../../../api/rulesTypes';
import { MAX_RULES, SIDE_EFFECT_KINDS } from './constants';
import type { DraftRuleInput, DryRunRequest, TesterDraft, TesterRule, TesterScope } from './types';

export interface RulesPayload {
  rules: DraftRuleInput[];
  described: TesterRule[];
  draftIndex: number | null;
}

const READ_ONLY_FIELDS: readonly string[] = ['id', 'position', 'created_at', 'updated_at'];
const ON_MATCH_STOP = 'stop';

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function inputOfRule(rule: Rule): DraftRuleInput {
  const input: DraftRuleInput = {};
  for (const [key, value] of Object.entries(rule)) {
    if (!READ_ONLY_FIELDS.includes(key)) {
      input[key] = value;
    }
  }
  return input;
}

export function stepsOf(input: DraftRuleInput): Record<string, unknown>[] {
  const steps: unknown = input.steps;
  return Array.isArray(steps) ? steps.filter(isRecord) : [];
}

export function describeRule(input: DraftRuleInput): TesterRule {
  const steps = stepsOf(input);
  return {
    name: typeof input.name === 'string' ? input.name : '',
    stepKinds: steps.map((step) => (typeof step.kind === 'string' ? step.kind : '')),
    stepNames: steps.map((step) => (typeof step.name === 'string' ? step.name : null)),
    stopsOnMatch: input.on_match === ON_MATCH_STOP,
  };
}

function isEnabled(input: DraftRuleInput): boolean {
  return input.enabled !== false;
}

export function buildRules(
  scope: TesterScope,
  draft: TesterDraft | null,
  saved: readonly Rule[] | undefined,
): RulesPayload | null {
  if (scope === 'rule') {
    if (draft === null) {
      return null;
    }
    return { rules: [draft.input], described: [describeRule(draft.input)], draftIndex: 0 };
  }
  if (saved === undefined) {
    return null;
  }
  const inputs: DraftRuleInput[] = [];
  let draftIndex: number | null = null;
  let substituted = false;
  for (const rule of saved) {
    if (draft !== null && draft.ruleId === rule.id) {
      substituted = true;
      if (isEnabled(draft.input)) {
        draftIndex = inputs.length;
        inputs.push(draft.input);
      }
    } else if (rule.enabled) {
      inputs.push(inputOfRule(rule));
    }
  }
  if (draft !== null && !substituted && isEnabled(draft.input)) {
    draftIndex = inputs.length;
    inputs.push(draft.input);
  }
  const rules = inputs.slice(0, MAX_RULES);
  return { rules, described: rules.map(describeRule), draftIndex };
}

export function dryRunRequestOf(captureId: string, rules: readonly DraftRuleInput[]): DryRunRequest {
  return { capture_id: captureId, rules: [...rules], execute_http: false };
}

export function sideEffectKinds(input: DraftRuleInput | null): string[] {
  if (input === null) {
    return [];
  }
  const kinds = stepsOf(input)
    .map((step) => (typeof step.kind === 'string' ? step.kind : ''))
    .filter((kind) => SIDE_EFFECT_KINDS.includes(kind));
  return [...new Set(kinds)];
}
