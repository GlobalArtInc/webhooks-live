import { describe, expect, it } from 'vitest';
import { TESTER_DRAFT_INPUT, TESTER_PAID_CAPTURE, TESTER_RULE_IDS, TESTER_SAVED_RULES } from './fixtures';
import { buildRules, dryRunRequestOf, sideEffectKinds } from './request';
import { realRunTarget } from './transport';
import type { TesterDraft } from './types';

const DRAFT: TesterDraft = {
  ruleId: TESTER_RULE_IDS.reject,
  input: { name: 'Edited', enabled: true, condition: {}, steps: [] },
};

describe('buildRules', () => {
  it('sends only the draft when testing one rule', () => {
    const payload = buildRules('rule', { ruleId: null, input: TESTER_DRAFT_INPUT }, undefined);

    expect(payload?.rules).toEqual([TESTER_DRAFT_INPUT]);
    expect(payload?.draftIndex).toBe(0);
    expect(buildRules('rule', null, undefined)).toBeNull();
  });

  it('puts the draft in place of its saved rule and drops read-only fields of the others', () => {
    const payload = buildRules('all', DRAFT, TESTER_SAVED_RULES);

    expect(payload?.rules.map((rule) => rule.name)).toEqual(['Mark paid orders', 'Edited', 'Mail to chat']);
    expect(payload?.draftIndex).toBe(1);
    expect(payload?.rules.every((rule) => !('id' in rule) && !('position' in rule) && !('created_at' in rule))).toBe(
      true,
    );
  });

  it('appends a draft that is not saved yet and skips disabled rules', () => {
    const disabled = TESTER_SAVED_RULES.map((rule) =>
      rule.id === TESTER_RULE_IDS.mail ? { ...rule, enabled: false } : rule,
    );

    const payload = buildRules('all', { ruleId: null, input: { name: 'Fresh', steps: [] } }, disabled);

    expect(payload?.rules.map((rule) => rule.name)).toEqual(['Mark paid orders', 'Reject unsigned calls', 'Fresh']);
    expect(payload?.draftIndex).toBe(2);
    expect(buildRules('all', DRAFT, undefined)).toBeNull();
  });

  it('leaves a disabled draft out of the run', () => {
    const payload = buildRules('all', { ...DRAFT, input: { ...DRAFT.input, enabled: false } }, TESTER_SAVED_RULES);

    expect(payload?.rules.map((rule) => rule.name)).toEqual(['Mark paid orders', 'Mail to chat']);
    expect(payload?.draftIndex).toBeNull();
  });
});

describe('request helpers', () => {
  it('never asks the server to execute outbound requests', () => {
    expect(dryRunRequestOf('capture-1', [TESTER_DRAFT_INPUT])).toEqual({
      capture_id: 'capture-1',
      rules: [TESTER_DRAFT_INPUT],
      execute_http: false,
    });
  });

  it('lists each side-effect kind of a rule once', () => {
    expect(sideEffectKinds(TESTER_DRAFT_INPUT)).toEqual(['http', 'increment']);
    expect(sideEffectKinds(null)).toEqual([]);
  });

  it('aims the real run at the endpoint URL with the captured sub-path and query', () => {
    expect(realRunTarget('https://hooks.example.test/abc/', TESTER_PAID_CAPTURE)).toBe(
      'https://hooks.example.test/abc/stripe?source=demo',
    );
  });
});
