import { describe, expect, it } from 'vitest';
import { ApiError } from '../../api/client';
import { failureOf, waitSeconds } from './InviteDialog';

function apiError(status: number, code: string | null, fieldCode?: string, retryAfter: number | null = null): ApiError {
  const details = fieldCode === undefined ? [] : [{ field: 'email', code: fieldCode, message: 'refused' }];
  return new ApiError({ status, code, message: 'failed', details, retryAfter });
}

describe('invite failures', () => {
  it('maps a 422 by the field code instead of calling every refusal the owner', () => {
    expect(failureOf(apiError(422, 'validation_failed', 'invalid_format'))).toBe('email');
    expect(failureOf(apiError(422, 'validation_failed', 'not_allowed'))).toBe('owner');
    expect(failureOf(apiError(422, 'validation_failed'))).toBe('failed');
  });

  it('maps the other statuses and reads Retry-After for 429', () => {
    expect(failureOf(apiError(404, 'user_not_found'))).toBe('unknown');
    expect(failureOf(apiError(403, 'forbidden'))).toBe('forbidden');
    expect(failureOf(apiError(429, 'rate_limited', undefined, 12))).toBe('limited');
    expect(waitSeconds(apiError(429, 'rate_limited', undefined, 12))).toBe(12);
    expect(waitSeconds(apiError(429, 'rate_limited'))).toBe(30);
    expect(failureOf(new TypeError('offline'))).toBe('failed');
  });
});
