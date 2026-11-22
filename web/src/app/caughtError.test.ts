import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../api/client';
import { isHandledError, logCaughtError } from './caughtError';

function apiError(status: number, code: string | null): ApiError {
  return new ApiError({ status, code, message: 'failed', details: [], retryAfter: null });
}

describe('caught error logging', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('treats the states the screens render as handled', () => {
    expect(isHandledError(apiError(410, 'endpoint_gone'))).toBe(true);
    expect(isHandledError(apiError(404, 'not_found'))).toBe(true);
    expect(isHandledError(apiError(403, 'forbidden'))).toBe(true);
    expect(isHandledError(apiError(401, 'unauthenticated'))).toBe(true);
  });

  it('keeps unexpected errors unhandled', () => {
    expect(isHandledError(apiError(500, 'internal_error'))).toBe(false);
    expect(isHandledError(apiError(401, 'grant_required'))).toBe(false);
    expect(isHandledError(apiError(403, null))).toBe(false);
    expect(isHandledError(new TypeError('boom'))).toBe(false);
  });

  it('does not log a handled error and logs an unexpected one with its component stack', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => undefined);

    logCaughtError(apiError(410, 'endpoint_gone'), { componentStack: 'at Screen' });
    expect(log).not.toHaveBeenCalled();

    const failure = new TypeError('boom');
    logCaughtError(failure, { componentStack: 'at Screen' });
    expect(log).toHaveBeenCalledWith(failure, 'at Screen');
  });
});
