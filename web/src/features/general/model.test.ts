import { describe, expect, it } from 'vitest';
import type { Endpoint, Instance } from '../../api/types';
import { endpointFixture } from '../../test/fixtures/endpoint';
import { instanceFixture } from '../../test/fixtures/instance';
import {
  draftOf,
  isLifetimeValid,
  isMaxCapturesValid,
  lifetimeOptions,
  limitsOf,
  meterState,
  patchFor,
  type GeneralLimits,
} from './model';

const DAY = 86400;
const HOUR = 3600;

const LIMITS: GeneralLimits = {
  defaultRetentionSeconds: 7 * DAY,
  maxRetentionSeconds: 30 * DAY,
  maxCaptures: 500,
  neverAllowed: true,
};

function sliding(seconds: number | null): Endpoint {
  return { ...endpointFixture, retention: { mode: 'sliding', seconds }, max_captures: 100, capture_ttl_seconds: null };
}

describe('lifetime options', () => {
  it('drops presets above the instance maximum and keeps a custom current value', () => {
    const options = lifetimeOptions(7 * DAY, 2 * DAY);
    expect(options.map((option) => option.seconds)).toEqual([HOUR, DAY, 7 * DAY, 2 * DAY]);
    expect(options.at(-1)?.label).toBe('wl.general.lifetime.preset.custom');
  });

  it('accepts sliding windows from one hour up to the maximum only', () => {
    const draft = draftOf(sliding(30 * DAY));
    expect(isLifetimeValid({ ...draft, retentionSeconds: HOUR - 1 }, LIMITS)).toBe(false);
    expect(isLifetimeValid({ ...draft, retentionSeconds: 31 * DAY }, LIMITS)).toBe(false);
    expect(isLifetimeValid({ ...draft, retentionSeconds: HOUR }, LIMITS)).toBe(true);
    expect(isLifetimeValid({ ...draft, retentionMode: 'never' }, LIMITS)).toBe(true);
  });
});

describe('capture limit', () => {
  it('accepts whole numbers from one up to the instance limit', () => {
    expect(isMaxCapturesValid('1', LIMITS)).toBe(true);
    expect(isMaxCapturesValid('500', LIMITS)).toBe(true);
    expect(isMaxCapturesValid('0', LIMITS)).toBe(false);
    expect(isMaxCapturesValid('501', LIMITS)).toBe(false);
    expect(isMaxCapturesValid('12a', LIMITS)).toBe(false);
  });
});

describe('save payload', () => {
  it('sends only title and labels for an editor, never the lifetime', () => {
    const source = sliding(7 * DAY);
    const draft = { ...draftOf(source), title: '  Renamed  ', retentionSeconds: DAY, maxCaptures: '5' };
    expect(patchFor(draft, source, false)).toEqual({ title: 'Renamed', tags: [] });
  });

  it('sends the chosen window for the owner and nothing unchanged', () => {
    const source = sliding(7 * DAY);
    const changed = { ...draftOf(source), retentionSeconds: DAY };
    expect(patchFor(changed, source, true)).toMatchObject({ retention: { mode: 'sliding', seconds: DAY } });
    expect(patchFor(draftOf(source), source, true)).not.toHaveProperty('retention');
  });

  it('sends keep-until-deleted with null seconds and the capture limits when they change', () => {
    const source = sliding(7 * DAY);
    const draft = { ...draftOf(source), retentionMode: 'never' as const, maxCaptures: '250', captureTtl: HOUR };
    expect(patchFor(draft, source, true)).toMatchObject({
      retention: { mode: 'never', seconds: null },
      max_captures: 250,
      capture_ttl_seconds: HOUR,
    });
  });
});

describe('instance limits', () => {
  it('allows keep-until-deleted only to a signed-in user when the instance allows it', () => {
    const signedIn: Instance = {
      ...instanceFixture,
      user: { id: 'user-1', email: 'owner@example.test', name: 'Owner' },
      limits: { ...instanceFixture.limits, never_expire_endpoints: 3 },
    };
    expect(limitsOf(signedIn).neverAllowed).toBe(true);
    expect(limitsOf({ ...signedIn, user: null }).neverAllowed).toBe(false);
    expect(limitsOf({ ...signedIn, limits: { ...signedIn.limits, never_expire_endpoints: 0 } }).neverAllowed).toBe(
      false,
    );
  });
});

describe('usage meters', () => {
  it('marks a meter near its limit at 90 percent and full at 100 percent', () => {
    expect(meterState(89, 100)).toBe('normal');
    expect(meterState(90, 100)).toBe('near');
    expect(meterState(100, 100)).toBe('full');
    expect(meterState(5, null)).toBe('unlimited');
  });
});
