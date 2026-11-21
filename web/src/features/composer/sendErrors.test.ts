import { describe, expect, it } from 'vitest';
import { ApiError } from '../../api/client';
import { sendResultFixture } from '../../test/fixtures/composer';
import { failureOf, failureOfResult, historyFailureOf } from './sendErrors';

function apiError(overrides: Partial<ConstructorParameters<typeof ApiError>[0]>): ApiError {
  return new ApiError({
    status: 422,
    code: 'validation_failed',
    message: 'failed',
    details: [],
    retryAfter: null,
    ...overrides,
  });
}

describe('composer send failures', () => {
  it('lands a refused target on the URL field with the server reason', () => {
    const failure = failureOf(
      apiError({ details: [{ field: 'url', code: 'target_refused', message: 'address 10.0.0.5 is private' }] }),
    );

    expect(failure).toEqual({
      kind: 'url',
      issue: { code: 'targetRefused', detail: 'address 10.0.0.5 is private' },
    });
  });

  it('keeps the quota limit name, its maximum and the retry delay', () => {
    const failure = failureOf(
      apiError({
        status: 429,
        code: 'quota_exceeded',
        retryAfter: 120,
        limit: { name: 'composer_user_hour', max: 30 },
      }),
    );

    expect(failure).toEqual({
      kind: 'quota',
      limit: { name: 'composer_user_hour', max: 30 },
      retryAfter: 120,
    });
  });

  it('reports a disabled composer only for the not_found answer', () => {
    expect(failureOf(apiError({ status: 404, code: 'not_found' }))).toEqual({ kind: 'disabled' });
    expect(failureOf(apiError({ status: 404, code: 'capture_not_found' }))).toEqual({ kind: 'failed' });
  });

  it('maps the timeout, body size, redirect and overload answers to their own kinds', () => {
    expect(failureOf(apiError({ status: 408, code: 'request_timeout' }))).toEqual({ kind: 'timeout' });
    expect(failureOf(apiError({ status: 413, code: 'payload_too_large' }))).toEqual({ kind: 'bodyTooLarge' });
    expect(failureOf(apiError({ status: 422, code: 'hop_limit' }))).toEqual({ kind: 'hopLimit' });
    expect(failureOf(apiError({ status: 429, code: 'rate_limited', retryAfter: 2 }))).toEqual({
      kind: 'rateLimited',
      retryAfter: 2,
    });
  });

  it('treats an aborted request as cancelled and a connection failure as a URL issue', () => {
    expect(failureOf(new DOMException('The operation was aborted.', 'AbortError'))).toEqual({ kind: 'cancelled' });
    expect(failureOfResult({ ...sendResultFixture, status: null, body: null, error: 'connection refused' })).toEqual({
      kind: 'url',
      issue: { code: 'unreachable', detail: 'connection refused' },
    });
    expect(failureOfResult(sendResultFixture)).toBeNull();
  });

  it('asks the user to sign in when history answers unauthenticated', () => {
    expect(historyFailureOf(apiError({ status: 401, code: 'unauthenticated' }))).toBe('signIn');
    expect(historyFailureOf(new Error('offline'))).toBe('failed');
  });
});
