import { describe, expect, it } from 'vitest';
import { RULE_CATALOGUE } from './catalogue';
import { draftToWire, inputKey, isScriptRuleInput, parseRuleInput, validateRule } from './model';

const RULE = {
  name: 'Mark paid',
  condition: { all: [{ value: 'request.header.x-event', op: 'eq', arg: 'paid' }] },
  steps: [
    { kind: 'extract', from: 'request.body', json_path: '$.amount', into: 'amount' },
    { kind: 'respond', status: '200', headers: [], body: 'ok' },
    { kind: 'set_header', name: 'X-Seen', value: '1' },
  ],
};

describe('rule draft wire mapping', () => {
  it('round trips a rule through the draft without changing its meaning', () => {
    const parsed = parseRuleInput(RULE, RULE_CATALOGUE);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) {
      return;
    }
    const wire = draftToWire(parsed.value, RULE_CATALOGUE).input;
    expect(wire.name).toBe('Mark paid');
    expect(wire.condition).toEqual(RULE.condition);
    expect(wire.steps).toEqual([
      { kind: 'extract', from: 'request.body', json_path: '$.amount', into: 'amount' },
      { kind: 'respond', status: '200', headers: [], body: 'ok' },
      { kind: 'set_header', name: 'X-Seen', value: '1' },
    ]);
  });

  it('refuses to edit a step kind or field it cannot represent instead of dropping data', () => {
    const unknownKind = parseRuleInput({ ...RULE, steps: [{ kind: 'teleport' }] }, RULE_CATALOGUE);
    const unknownField = parseRuleInput(
      { ...RULE, steps: [{ kind: 'forward', target_url: 'https://a.test', header_allowlist: ['x'] }] },
      RULE_CATALOGUE,
    );
    expect(unknownKind.ok).toBe(false);
    expect(unknownField.ok).toBe(false);
  });

  it('keeps the same revision key for equal content and a different one after an edit', () => {
    const edited = { ...RULE, name: 'Mark paid orders' };
    expect(inputKey(RULE, RULE_CATALOGUE)).toBe(inputKey({ ...RULE }, RULE_CATALOGUE));
    expect(inputKey(edited, RULE_CATALOGUE)).not.toBe(inputKey(RULE, RULE_CATALOGUE));
  });

  it('recognises a rule that is one script step', () => {
    expect(isScriptRuleInput({ steps: [{ kind: 'script', source: '' }] })).toBe(true);
    expect(isScriptRuleInput(RULE)).toBe(false);
  });

  it('reports a missing name, a missing step and an empty required field', () => {
    const parsed = parseRuleInput({ name: ' ', steps: [{ kind: 'extract', from: '', into: 'a' }] }, RULE_CATALOGUE);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) {
      return;
    }
    const withStep = validateRule(parsed.value, RULE_CATALOGUE);
    expect(withStep.nameMissing).toBe(true);
    expect(withStep.steps.map((problem) => `${problem.kind}:${problem.field}`)).toContain('required:from');
    const empty = validateRule({ ...parsed.value, steps: [] }, RULE_CATALOGUE);
    expect(empty.stepsMissing).toBe(true);
  });
});
