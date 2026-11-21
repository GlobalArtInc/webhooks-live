import { describe, expect, it } from 'vitest';
import { scheduleFixture } from '../../test/fixtures/schedules';
import { cadenceToExpression, cronToCadence } from './cadence';
import { blankScheduleDraft, scheduleDraftOf, scheduleInputOf, validateScheduleDraft } from './scheduleDraft';

const FIVE_MINUTES = 300;

describe('cadence mapping', () => {
  it('maps the three cadence kinds to cron and back', () => {
    expect(cadenceToExpression({ kind: 'interval', every: 15, unit: 'minutes' })).toBe('*/15 * * * *');
    expect(cadenceToExpression({ kind: 'interval', every: 6, unit: 'hours' })).toBe('0 */6 * * *');
    expect(cadenceToExpression({ kind: 'daily', time: '02:30' })).toBe('30 2 * * *');
    expect(cronToCadence('30 2 * * *')).toEqual({ kind: 'daily', time: '02:30' });
    expect(cronToCadence('0 */6 * * *')).toEqual({ kind: 'interval', every: 6, unit: 'hours' });
    expect(cronToCadence('0 9 * * 1-5')).toEqual({ kind: 'expression', expression: '0 9 * * 1-5' });
  });
});

describe('scheduleDraft', () => {
  it('round-trips a schedule through the draft into the save input', () => {
    const input = scheduleInputOf(scheduleDraftOf(scheduleFixture));
    expect(input).toEqual({
      name: 'Ping staging',
      cron: '*/5 * * * *',
      timezone: 'UTC',
      request: {
        method: 'GET',
        url: 'https://staging.example.test/health',
        headers: [{ name: 'Authorization', value: 'Bearer {{ secret.EXAMPLE_TOKEN }}' }],
        body: null,
      },
    });
  });

  it('flags an interval below the instance minimum and a blank name', () => {
    const draft = {
      ...blankScheduleDraft(),
      cadence: { kind: 'interval' as const, every: 1, unit: 'minutes' as const },
    };
    const issues = validateScheduleDraft(draft, FIVE_MINUTES);
    expect(issues.get('every')?.code).toBe('interval_too_short');
    expect(issues.get('name')?.code).toBe('required');
  });
});
