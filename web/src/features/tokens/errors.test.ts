import { describe, expect, it } from 'vitest';
import { ApiError } from '../../api/client';
import { describeTokenError, retryTime, type TokenErrorInfo } from './errors';

function failure(status: number, code: string | null, extra: Partial<ConstructorParameters<typeof ApiError>[0]> = {}) {
  return new ApiError({ status, code, message: 'failed', details: [], retryAfter: null, ...extra });
}

describe('describeTokenError', () => {
  it('maps the active token limit with its maximum', () => {
    const error = failure(422, 'limit_reached', { limit: { name: 'tokens', max: 20, window: null, resets_at: null } });

    expect(describeTokenError(error)).toMatchObject({ kind: 'limit', max: 20 });
  });

  it('maps the hourly creation quota with its reset time', () => {
    const resets = '2026-11-21T11:00:00.000Z';
    const error = failure(429, 'quota_exceeded', {
      retryAfter: 900,
      limit: { name: 'token_creation', max: 10, window: 'hour', resets_at: resets },
    });

    expect(describeTokenError(error)).toMatchObject({ kind: 'rate', max: 10, resetsAt: resets, retryAfter: 900 });
  });

  it('maps session errors on 401 and 403', () => {
    expect(describeTokenError(failure(401, 'unauthenticated')).kind).toBe('session');
    expect(describeTokenError(failure(403, 'session_required')).kind).toBe('session');
  });

  it('keeps server field errors', () => {
    const error = failure(422, 'validation_failed', { details: [{ field: 'name', message: 'is invalid' }] });

    expect(describeTokenError(error)).toMatchObject({
      kind: 'fields',
      fields: [{ field: 'name', message: 'is invalid' }],
    });
  });

  it('flags 5xx answers and non-API failures as possibly created, and 4xx answers as not created', () => {
    expect(describeTokenError(failure(502, null)).maybeCreated).toBe(true);
    expect(describeTokenError(new TypeError('network')).maybeCreated).toBe(true);
    expect(describeTokenError(new DOMException('aborted', 'AbortError')).maybeCreated).toBe(true);
    expect(describeTokenError(failure(422, 'limit_reached')).maybeCreated).toBe(false);
    expect(describeTokenError(failure(403, 'forbidden')).maybeCreated).toBe(false);
  });

  it('falls back to a generic error for anything else', () => {
    expect(describeTokenError(failure(503, 'store_unavailable')).kind).toBe('generic');
    expect(describeTokenError(new TypeError('network')).kind).toBe('generic');
  });
});

describe('retryTime', () => {
  const info: TokenErrorInfo = {
    kind: 'rate',
    max: 10,
    resetsAt: null,
    retryAfter: null,
    fields: [],
    maybeCreated: false,
  };

  it('prefers resets_at, then Retry-After, then nothing', () => {
    const now = Date.parse('2026-11-21T10:00:00.000Z');

    expect(retryTime({ ...info, resetsAt: '2026-11-21T11:00:00.000Z', retryAfter: 5 }, now)).toBe(
      Date.parse('2026-11-21T11:00:00.000Z'),
    );
    expect(retryTime({ ...info, retryAfter: 60 }, now)).toBe(now + 60_000);
    expect(retryTime(info, now)).toBeNull();
  });
});
