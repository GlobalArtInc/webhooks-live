import { describe, expect, it } from 'vitest';
import { clampCursor, freshCursor, isRevealed, resetCursor, selectAt, stepBack, stepForward } from './cursor';
import { simulateDryRun } from './fixtureEngine';
import { TESTER_DRAFT_INPUT, TESTER_PAID_CAPTURE, TESTER_SAVED_RULES } from './fixtures';
import { parseRuleRun } from './parse';
import { describeRule, inputOfRule } from './request';
import { buildTrace, errorPosition, findStepItem } from './trace';
import type { Capture } from '../../../api/types';
import type { DraftRuleInput, TraceModel } from './types';

function traceOf(rules: DraftRuleInput[], capture: Capture = TESTER_PAID_CAPTURE, extended = false): TraceModel {
  const run = parseRuleRun(simulateDryRun(rules, capture, { extended }));
  return buildTrace(run, rules.map(describeRule));
}

describe('buildTrace', () => {
  it('lists the conditions entry and every step with its status', () => {
    const model = traceOf([TESTER_DRAFT_INPUT]);

    expect(model.items.map((item) => `${item.type}:${item.status}`)).toEqual([
      'rule:matched',
      'step:ran',
      'step:ran',
      'step:deferred',
      'step:simulated',
    ]);
    expect(model.final.status).toBe(200);
    expect(model.simulatedCount).toBe(1);
    expect(model.stepCount).toBe(4);
  });

  it('adds not-reached placeholders for steps after a failure and reports the position', () => {
    const failing: DraftRuleInput = {
      name: 'boom rule',
      condition: {},
      steps: [
        { kind: 'respond', status: '200', content_type: 'text/plain', headers: [], body: 'x' },
        { kind: 'script', source: 'boom' },
        { kind: 'respond', status: '500', content_type: 'text/plain', headers: [], body: 'y' },
      ],
    };

    const model = traceOf([failing]);

    expect(model.failed).toBe(true);
    expect(model.error).toContain('boom');
    expect(model.final.status).toBeNull();
    expect(model.items.map((item) => item.status)).toEqual(['matched', 'ran', 'failed', 'notReached']);
    expect(model.items[3]?.placeholder).toBe(true);
    expect(model.groups[0]?.failedPosition).toBe(2);
    expect(errorPosition(model.items[2]?.detail ?? null)).toEqual({ line: 2, column: 5 });
  });

  it('marks a rule that did not match and shows no steps for it', () => {
    const model = traceOf([
      { name: 'never', condition: { value: 'request.method', op: 'eq', arg: 'DELETE' }, steps: [{ kind: 'stop' }] },
    ]);

    expect(model.items.map((item) => item.status)).toEqual(['notMatched']);
    expect(model.matchedCount).toBe(0);
  });

  it('shows where a stop cut the chain across all rules and which rule answered', () => {
    const rules = TESTER_SAVED_RULES.map(inputOfRule);

    const model = traceOf(rules);

    expect(model.groups.map((group) => group.matched)).toEqual([true, true, null]);
    expect(model.groups[1]?.cut).toBe('stop');
    expect(model.groups[2]?.ruleName).toBe('Mail to chat');
    expect(model.final.status).toBe(401);
    expect(model.final.groupIndex).toBe(1);
  });

  it('keeps the condition results and prefers the reported response when the server sends them', () => {
    const model = traceOf([TESTER_DRAFT_INPUT], TESTER_PAID_CAPTURE, true);

    expect(model.items[0]?.conditions).toHaveLength(2);
    expect(model.items[0]?.conditions?.every((condition) => condition.result)).toBe(true);
    expect(model.final.detail?.body.data).toBe('{"ok":true}');
    expect(findStepItem(model, 1, 0)?.stepKind).toBe('respond');
  });
});

describe('errorPosition', () => {
  it('reads the common position spellings and nothing else', () => {
    expect(errorPosition('script: error at line 3, column 9')).toEqual({ line: 3, column: 9 });
    expect(errorPosition('Rhai error (4:12)')).toEqual({ line: 4, column: 12 });
    expect(errorPosition('failed at line 7')).toEqual({ line: 7, column: 1 });
    expect(errorPosition('timeout after 5 s')).toBeNull();
    expect(errorPosition(null)).toBeNull();
  });
});

describe('stepping cursor', () => {
  it('walks forward and back, reveals only up to the cursor and resets to the first entry', () => {
    let cursor = freshCursor(1);
    expect(isRevealed(cursor, 3)).toBe(true);

    cursor = stepForward(cursor, 3);
    expect(cursor).toEqual({ resultId: 1, selected: 1, revealedTo: 1 });
    expect(isRevealed(cursor, 2)).toBe(false);
    expect(selectAt(cursor, 3, 3)).toBe(cursor);

    cursor = stepForward(stepForward(stepForward(cursor, 3), 3), 3);
    expect(cursor.selected).toBe(3);
    cursor = stepBack(cursor);
    expect(cursor).toMatchObject({ selected: 2, revealedTo: 2 });
    expect(resetCursor(cursor)).toMatchObject({ selected: 0, revealedTo: 0 });
  });

  it('clamps a kept cursor into a shorter trace', () => {
    expect(clampCursor({ resultId: 1, selected: 5, revealedTo: 5 }, 2, 2)).toEqual({
      resultId: 2,
      selected: 2,
      revealedTo: 2,
    });
  });
});
