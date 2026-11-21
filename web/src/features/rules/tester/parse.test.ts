import { describe, expect, it } from 'vitest';
import { MAX_ENTRIES, TEXT_LIMIT_CHARS } from './constants';
import { boundJson, parseRuleRun, UnreadableResultError } from './parse';

const RULE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f7a51';

function wire(entries: unknown[], extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    id: 'run-1',
    capture_id: 'capture-1',
    trigger: 'dry_run',
    state: 'succeeded',
    rules_version: 3,
    response_status: 202,
    error: null,
    entries,
    created_at: '2026-11-21T10:00:00.000Z',
    finished_at: '2026-11-21T10:00:00.010Z',
    ...extra,
  };
}

describe('parseRuleRun', () => {
  it('reads the rule and step entries of the dry run contract', () => {
    const result = parseRuleRun(
      wire([
        { rule_id: RULE_ID, rule_name: 'answer', matched: true },
        {
          rule_id: RULE_ID,
          step: 0,
          name: null,
          kind: 'increment',
          outcome: 'no_op',
          duration_ms: 1,
          detail: 'would add 2',
          output: { counter: 'hits', value: 2 },
        },
        {
          rule_id: RULE_ID,
          step: 1,
          name: 'late',
          kind: 'notify',
          outcome: 'deferred',
          duration_ms: 0,
          detail: 'runs after the response',
          output: null,
        },
      ]),
    );

    expect(result.responseStatus).toBe(202);
    expect(result.rulesVersion).toBe(3);
    expect(result.response).toBeNull();
    expect(result.entries.map((entry) => entry.type)).toEqual(['rule', 'step', 'step']);
    const step = result.entries[1];
    expect(step?.type === 'step' && step.outcome).toBe('no_op');
    expect(step?.type === 'step' && step.output).toEqual({ counter: 'hits', value: 2 });
  });

  it('drops entries it cannot read and unknown outcomes', () => {
    const result = parseRuleRun(
      wire([
        { rule_id: RULE_ID, rule_name: 'ok', matched: false },
        { rule_id: RULE_ID, step: 0, kind: 'respond', outcome: 'exploded' },
        { step: 1, kind: 'respond', outcome: 'ok' },
        'text',
        null,
      ]),
    );

    expect(result.entries).toHaveLength(1);
  });

  it('rejects a payload that is not a run', () => {
    expect(() => parseRuleRun({ nope: true })).toThrow(UnreadableResultError);
    expect(() => parseRuleRun(null)).toThrow(UnreadableResultError);
    expect(() => parseRuleRun({ entries: 'x' })).toThrow(UnreadableResultError);
  });

  it('bounds the entry count and the length of every text', () => {
    const many = Array.from({ length: MAX_ENTRIES + 5 }, () => ({
      rule_id: RULE_ID,
      rule_name: 'x'.repeat(TEXT_LIMIT_CHARS * 2),
      matched: true,
    }));

    const result = parseRuleRun(wire(many));

    expect(result.entries).toHaveLength(MAX_ENTRIES);
    expect(result.entriesTruncated).toBe(true);
    const first = result.entries[0];
    expect(first?.type === 'rule' && first.ruleName.length).toBeLessThan(TEXT_LIMIT_CHARS + 10);
  });

  it('reads the optional condition results, variable snapshots and response when a server sends them', () => {
    const result = parseRuleRun(
      wire(
        [
          {
            rule_id: RULE_ID,
            rule_name: 'r',
            matched: true,
            conditions: [
              { subject: 'request.method', op: 'eq', expected: 'POST', actual: 'POST', result: true },
              { result: 'yes' },
            ],
          },
          {
            rule_id: RULE_ID,
            step: 0,
            name: null,
            kind: 'set_var',
            outcome: 'ok',
            duration_ms: 1,
            detail: null,
            output: null,
            vars: { a: '1' },
          },
        ],
        {
          response: {
            status: 201,
            content_type: 'text/plain',
            headers: [{ name: 'X-A', value: 'b' }, { name: 1 }],
            body: { encoding: 'utf8', data: 'hi', truncated: false },
            body_size: 2,
          },
        },
      ),
    );

    const rule = result.entries[0];
    expect(rule?.type === 'rule' && rule.conditions).toHaveLength(1);
    const step = result.entries[1];
    expect(step?.type === 'step' && step.vars).toEqual({ a: '1' });
    expect(result.response?.headers).toEqual([{ name: 'X-A', value: 'b' }]);
    expect(result.response?.bodySize).toBe(2);
  });
});

describe('boundJson', () => {
  it('cuts deep and wide values instead of walking them', () => {
    let deep: unknown = 'leaf';
    for (let level = 0; level < 20; level += 1) {
      deep = { inner: deep };
    }
    const wide = Array.from({ length: 5000 }, (_, index) => index);

    expect(JSON.stringify(boundJson(deep))).toContain('...');
    expect(JSON.stringify(boundJson(wide)).length).toBeLessThan(5000);
  });
});
