import { describe, expect, it } from 'vitest';
import { meterFixture } from '../../test/fixtures/usage';
import { fillPercent, meterLevel } from './meterLevel';

describe('meterLevel', () => {
  it.each([
    [89, 'normal'],
    [90, 'near'],
    [99, 'near'],
    [100, 'full'],
  ] as const)('classifies %i of 100 as %s', (used, level) => {
    expect(meterLevel(meterFixture({ used, max: 100 }))).toBe(level);
  });

  it('caps the fill at the full width when usage is over the limit', () => {
    expect(fillPercent(meterFixture({ used: 150, max: 100 }))).toBe(100);
  });
});
