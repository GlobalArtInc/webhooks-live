import { describe, expect, it } from 'vitest';
import { simulateDryRun } from './fixtureEngine';
import { TESTER_DRAFT_INPUT, TESTER_HEALTH_CAPTURE, TESTER_PAID_CAPTURE } from './fixtures';
import { parseRuleRun } from './parse';
import { describeRule } from './request';
import { buildTrace } from './trace';
import { filterRows, requestSnapshot, variableRowsAt } from './variables';

function model(extended: boolean) {
  const run = parseRuleRun(simulateDryRun([TESTER_DRAFT_INPUT], TESTER_PAID_CAPTURE, { extended }));
  return buildTrace(run, [describeRule(TESTER_DRAFT_INPUT)]);
}

describe('variableRowsAt', () => {
  it('lists the request first, with nothing marked on the conditions entry', () => {
    const rows = variableRowsAt(model(false), TESTER_PAID_CAPTURE, 0);

    expect(rows.map((row) => row.path)).toEqual(
      expect.arrayContaining([
        'request.method',
        'request.path',
        'request.query.source',
        'request.header.x-demo-signature',
        'request.json.type',
        'request.json.data.object.amount',
      ]),
    );
    expect(rows.every((row) => row.change === 'same')).toBe(true);
  });

  it('marks a variable new on the step that produced it and unchanged after', () => {
    const trace = model(false);

    const at = variableRowsAt(trace, TESTER_PAID_CAPTURE, 1).find((row) => row.path === 'var.amount');
    const later = variableRowsAt(trace, TESTER_PAID_CAPTURE, 2).find((row) => row.path === 'var.amount');

    expect(at).toMatchObject({ text: '"2000"', change: 'new', group: 'var' });
    expect(later?.change).toBe('same');
  });

  it('follows simulated counters and prefers a snapshot the server reports', () => {
    const plain = variableRowsAt(model(false), TESTER_PAID_CAPTURE, 4);
    const reported = variableRowsAt(model(true), TESTER_PAID_CAPTURE, 4);

    expect(plain.find((row) => row.path === 'counter.paid')).toMatchObject({
      text: '1',
      change: 'new',
      group: 'counter',
    });
    expect(plain.find((row) => row.path === 'var.amount')?.text).toBe('"2000"');
    expect(reported.find((row) => row.path === 'var.amount')?.text).toBe('"2000"');
    expect(reported.find((row) => row.path === 'counter.paid')?.text).toBe('1');
  });

  it('shows no request rows without a capture and flattens only readable JSON', () => {
    expect(requestSnapshot(undefined).size).toBe(0);
    const health = requestSnapshot(TESTER_HEALTH_CAPTURE);
    expect([...health.keys()]).toEqual(['request.method', 'request.path', 'request.header.user-agent']);
  });
});

describe('filterRows', () => {
  it('filters by path ignoring case', () => {
    const rows = variableRowsAt(model(false), TESTER_PAID_CAPTURE, 1);

    expect(filterRows(rows, 'AMOUNT').map((row) => row.path)).toEqual([
      'request.json.data.object.amount',
      'var.amount',
    ]);
    expect(filterRows(rows, '  ')).toHaveLength(rows.length);
  });
});
