import { describe, expect, it } from 'vitest';
import { ApiError } from '../../../api/client';
import { phaseOfFailure } from './useConfirmation';

function failure(status: number): ApiError {
  return new ApiError({ status, code: null, message: 'failed', details: [], retryAfter: null });
}

describe('confirmation failure phase', () => {
  it('treats a client refusal as an invalid or expired link and everything else as retryable', () => {
    expect(phaseOfFailure(failure(422))).toBe('invalid');
    expect(phaseOfFailure(failure(404))).toBe('invalid');
    expect(phaseOfFailure(failure(429))).toBe('failed');
    expect(phaseOfFailure(failure(503))).toBe('failed');
    expect(phaseOfFailure(new TypeError('network down'))).toBe('failed');
  });
});
