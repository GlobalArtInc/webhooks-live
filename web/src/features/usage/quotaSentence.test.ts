import { describe, expect, it } from 'vitest';
import { ApiError } from '../../api/client';
import { createI18nInstance } from '../../i18n/setup';
import { quotaSentence } from './quotaSentence';

const NOW = Date.parse('2026-11-21T10:00:00.000Z');
const FIFTEEN_MINUTES_MS = 15 * 60_000;

const i18n = createI18nInstance();

function failure(init: Partial<ConstructorParameters<typeof ApiError>[0]>): ApiError {
  return new ApiError({ status: 500, code: null, message: 'failed', details: [], retryAfter: null, ...init });
}

describe('quotaSentence', () => {
  it('tells when a windowed quota resets', () => {
    const error = failure({
      status: 429,
      code: 'quota_exceeded',
      limit: {
        name: 'outbound',
        max: 200,
        window: 'hour',
        resets_at: new Date(NOW + FIFTEEN_MINUTES_MS).toISOString(),
      },
    });

    expect(quotaSentence(error, i18n.t, 'en', NOW)).toBe('Outbound requests limit reached. Try again in 15 minutes.');
  });

  it('names the full storage and asks for space to be freed', () => {
    const error = failure({ status: 507, code: 'quota_exceeded' });

    expect(quotaSentence(error, i18n.t, 'en', NOW)).toBe(
      'Storage is full. Delete captures or endpoints to free space.',
    );
  });

  it('states the allowed count for a count limit', () => {
    const error = failure({
      status: 422,
      code: 'limit_reached',
      limit: { name: 'tokens', max: 20, window: null, resets_at: null },
    });

    expect(quotaSentence(error, i18n.t, 'en', NOW)).toBe(
      'API tokens limit reached: 20 allowed. Remove one to add another.',
    );
  });

  it('returns nothing for errors that are not limits', () => {
    expect(quotaSentence(failure({ status: 500, code: 'store_unavailable' }), i18n.t, 'en', NOW)).toBeNull();
  });
});
