import { describe, expect, it } from 'vitest';
import {
  draftToInput,
  emptyDraft,
  expiryDate,
  hasProblems,
  pruneEndpointIds,
  toggleScope,
  validateDraft,
  writesEndpointsWhileRestricted,
  type TokenDraft,
} from './draft';
import { tokenState } from './tokenState';
import { tokenFixture } from './testFixtures';

function draftWith(overrides: Partial<TokenDraft>): TokenDraft {
  return { ...emptyDraft(), name: 'ci', ...overrides };
}

describe('validateDraft', () => {
  it('accepts the default draft once it has a name', () => {
    expect(hasProblems(validateDraft(draftWith({})))).toBe(false);
  });

  it('rejects an empty or blank name', () => {
    expect(validateDraft(draftWith({ name: '   ' })).name).toBe('required');
  });

  it('accepts a name of exactly 64 characters and rejects 65', () => {
    expect(validateDraft(draftWith({ name: 'a'.repeat(64) })).name).toBeUndefined();
    expect(validateDraft(draftWith({ name: 'a'.repeat(65) })).name).toBe('tooLong');
  });

  it('counts characters, not UTF-16 units', () => {
    expect(validateDraft(draftWith({ name: '\u{1F600}'.repeat(64) })).name).toBeUndefined();
  });

  it('rejects control characters in the name', () => {
    expect(validateDraft(draftWith({ name: 'a\u0007b' })).name).toBe('control');
  });

  it('requires at least one scope', () => {
    expect(validateDraft(draftWith({ scopes: [] })).scopes).toBe('required');
  });

  it('requires at least one endpoint when restricted and allows 50 but not 51', () => {
    const ids = (count: number) => Array.from({ length: count }, (_, index) => `endpoint-${index}`);
    expect(validateDraft(draftWith({ endpointMode: 'only', endpointIds: [] })).endpoints).toBe('required');
    expect(validateDraft(draftWith({ endpointMode: 'only', endpointIds: ids(50) })).endpoints).toBeUndefined();
    expect(validateDraft(draftWith({ endpointMode: 'only', endpointIds: ids(51) })).endpoints).toBe('tooMany');
  });
});

describe('draftToInput', () => {
  it('trims the name and leaves endpoint_ids out when all endpoints are allowed', () => {
    const input = draftToInput(draftWith({ name: '  ci  ', endpointIds: ['kept-out'] }));

    expect(input).toEqual({ name: 'ci', scopes: ['read'], expires_in_days: 90 });
  });

  it('sends the chosen endpoints and expiry', () => {
    const input = draftToInput(draftWith({ endpointMode: 'only', endpointIds: ['a', 'b'], days: 7 }));

    expect(input).toEqual({ name: 'ci', scopes: ['read'], endpoint_ids: ['a', 'b'], expires_in_days: 7 });
  });
});

describe('toggleScope', () => {
  it('keeps the catalogue order and never duplicates a scope', () => {
    const added = toggleScope(['send'], 'read', true);

    expect(added).toEqual(['read', 'send']);
    expect(toggleScope(added, 'read', true)).toEqual(['read', 'send']);
    expect(toggleScope(added, 'send', false)).toEqual(['read']);
  });
});

describe('expiryDate', () => {
  it('adds whole days', () => {
    expect(expiryDate(30, Date.parse('2026-11-21T00:00:00.000Z')).toISOString()).toBe('2026-12-21T00:00:00.000Z');
  });
});

describe('tokenState', () => {
  const now = Date.parse('2026-11-21T00:00:00.000Z');

  it('is revoked whenever revoked_at is set, even past expiry', () => {
    const token = tokenFixture({ revoked_at: '2026-11-01T00:00:00.000Z', expires_at: '2026-10-01T00:00:00.000Z' });

    expect(tokenState(token, now)).toBe('revoked');
  });

  it('is expired at the exact expiry instant and active before it', () => {
    expect(tokenState(tokenFixture({ expires_at: '2026-11-21T00:00:00.000Z' }), now)).toBe('expired');
    expect(tokenState(tokenFixture({ expires_at: '2026-11-21T00:00:01.000Z' }), now)).toBe('active');
  });
});

describe('pruneEndpointIds', () => {
  it('drops ids that are not in the loaded list', () => {
    const pruned = pruneEndpointIds(
      draftWith({ endpointMode: 'only', endpointIds: ['a', 'gone'] }),
      new Set(['a', 'b']),
    );

    expect(pruned.endpointIds).toEqual(['a']);
    expect(
      validateDraft(pruneEndpointIds(draftWith({ endpointMode: 'only', endpointIds: ['gone'] }), new Set())).endpoints,
    ).toBe('required');
  });
});

describe('writesEndpointsWhileRestricted', () => {
  it('is true only for endpoints:write together with an endpoint restriction', () => {
    const base = { scopes: ['read', 'endpoints:write'] } as const;

    expect(writesEndpointsWhileRestricted(draftWith({ ...base, scopes: [...base.scopes], endpointMode: 'only' }))).toBe(
      true,
    );
    expect(writesEndpointsWhileRestricted(draftWith({ ...base, scopes: [...base.scopes], endpointMode: 'all' }))).toBe(
      false,
    );
    expect(writesEndpointsWhileRestricted(draftWith({ endpointMode: 'only' }))).toBe(false);
  });
});
