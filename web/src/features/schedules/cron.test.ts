import { describe, expect, it } from 'vitest';
import { parseCron, nextRuns } from './cron';

const NOVEMBER_21_MORNING = Date.UTC(2026, 10, 21, 10, 7);

describe('parseCron', () => {
  it.each(['*/5 * * * *', '0 9 * * 1-5', '15,45 8-17/3 1 */2 0,7', '0 0 29 2 *'])('accepts %s', (expression) => {
    expect(parseCron(expression)).not.toBeNull();
  });

  it.each([
    ['too few fields', '*/5 * * *'],
    ['minute out of range', '60 * * * *'],
    ['zero step', '*/0 * * * *'],
    ['reversed range', '10-5 * * * *'],
    ['word instead of number', 'mon * * * *'],
  ])('rejects %s', (_label, expression) => {
    expect(parseCron(expression)).toBeNull();
  });
});

describe('nextRuns', () => {
  it('returns the next runs of a five-minute cadence in UTC', () => {
    const runs = nextRuns('*/15 * * * *', 'UTC', NOVEMBER_21_MORNING, 3);
    expect(runs).toEqual([
      Date.UTC(2026, 10, 21, 10, 15),
      Date.UTC(2026, 10, 21, 10, 30),
      Date.UTC(2026, 10, 21, 10, 45),
    ]);
  });

  it('evaluates a daily time in the chosen time zone', () => {
    const runs = nextRuns('0 2 * * *', 'Europe/Berlin', Date.UTC(2026, 10, 21, 0, 0), 1);
    expect(runs).toEqual([Date.UTC(2026, 10, 21, 1, 0)]);
  });

  it('returns null for an invalid expression or time zone', () => {
    expect(nextRuns('not a cron', 'UTC', NOVEMBER_21_MORNING, 5)).toBeNull();
    expect(nextRuns('*/5 * * * *', 'Mars/Olympus', NOVEMBER_21_MORNING, 5)).toBeNull();
  });
});
