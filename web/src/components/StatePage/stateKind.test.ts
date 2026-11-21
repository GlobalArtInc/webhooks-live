import { describe, expect, it } from 'vitest';
import { ApiError } from '../../api/client';
import { stateKindForError } from './stateKind';

function apiError(status: number, code: string | null): ApiError {
  return new ApiError({ status, code, message: 'failed', details: [], retryAfter: null });
}

describe('stateKindForError', () => {
  it.each([
    ['410 endpoint_gone', apiError(410, 'endpoint_gone'), 'gone'],
    ['404 not_found', apiError(404, 'not_found'), 'not-found'],
    ['401 unauthenticated', apiError(401, 'unauthenticated'), 'signin'],
    ['403 forbidden', apiError(403, 'forbidden'), 'not-shared'],
    ['403 csrf_rejected', apiError(403, 'csrf_rejected'), 'error'],
    ['403 not_available_in_mode', apiError(403, 'not_available_in_mode'), 'error'],
    ['500 without a code', apiError(500, null), 'error'],
    ['503 store_unavailable', apiError(503, 'store_unavailable'), 'error'],
    ['429 rate_limited', apiError(429, 'rate_limited'), 'error'],
    ['network failure', new TypeError('Failed to fetch'), 'error'],
    ['non-error value', 'boom', 'error'],
  ])('maps %s', (_name, error, expected) => {
    expect(stateKindForError(error)).toBe(expected);
  });
});
