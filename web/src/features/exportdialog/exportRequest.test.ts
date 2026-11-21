import { describe, expect, it } from 'vitest';
import { ApiError, captureExportPath } from '../../api/client';
import { exportQueryOf, failureOf, fileNameFrom, selectionFitsExport } from './exportRequest';

const ENDPOINT_ID = '0123456789abcdefghjkmnpq';

function apiError(status: number, code: string | null, retryAfter: number | null = null): ApiError {
  return new ApiError({ status, code, message: 'failed', details: [], retryAfter });
}

describe('export request helpers', () => {
  it('reads the server file name and replaces characters that are unsafe in a download', () => {
    expect(fileNameFrom('attachment; filename="webhooks-live-export.zip"', 'fallback.zip')).toBe(
      'webhooks-live-export.zip',
    );
    expect(fileNameFrom('attachment; filename="a b/c.json"', 'fallback.json')).toBe('a_b_c.json');
    expect(fileNameFrom(null, 'fallback.csv')).toBe('fallback.csv');
  });

  it('classifies busy, too large, gone and other failures with the retry hint', () => {
    expect(failureOf(apiError(429, 'rate_limited', 3))).toEqual({ kind: 'failed', reason: 'busy', retryAfter: 3 });
    expect(failureOf(apiError(422, 'export_too_large'))).toEqual({
      kind: 'failed',
      reason: 'tooLarge',
      retryAfter: null,
    });
    expect(failureOf(apiError(410, 'endpoint_gone'))).toEqual({ kind: 'failed', reason: 'gone', retryAfter: null });
    expect(failureOf(new Error('network'))).toEqual({ kind: 'failed', reason: 'failed', retryAfter: null });
  });

  it('builds the streamed GET from the list filters only and keeps unset filters out', () => {
    const query = exportQueryOf('csv', { method: 'POST', status: undefined, q: undefined });
    expect(captureExportPath(ENDPOINT_ID, query)).toBe(
      `/api/v1/endpoints/${ENDPOINT_ID}/export?format=csv&method=POST`,
    );
    expect(selectionFitsExport(0)).toBe(false);
    expect(selectionFitsExport(500)).toBe(true);
    expect(selectionFitsExport(501)).toBe(false);
  });
});
