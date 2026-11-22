import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';
import { ApiError } from '../../api/client';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { importHandlers, type ImportRecord } from '../../test/msw/imports';
import { importPath, failureOf, ImportNetworkError, sendImport, type ImportUpload } from './importRequest';

registerMockServer();

const IMPORT_INTO_PATH = `/api/v1/endpoints/${ENDPOINT_ID}/import`;

function upload(endpointId: string | null, keepTimes = true): ImportUpload {
  return {
    endpointId,
    keepTimes,
    format: 'json',
    file: new File(['{}'], 'export.json', { type: 'application/json' }),
    signal: new AbortController().signal,
    onProgress: () => undefined,
  };
}

function apiError(
  status: number,
  retryAfter: number | null = null,
  details: ApiError['details'] = [],
  code: string | null = null,
): ApiError {
  return new ApiError({ status, code, message: 'refused', details, retryAfter });
}

describe('importPath', () => {
  it('targets the endpoint with the keep_times switch', () => {
    expect(importPath(ENDPOINT_ID, false)).toBe(`${IMPORT_INTO_PATH}?keep_times=false`);
  });

  it('targets the new endpoint route when no endpoint is given', () => {
    expect(importPath(null, true)).toBe('/api/v1/endpoints/import?keep_times=true');
  });
});

describe('failureOf', () => {
  it('maps rate limiting to busy with the server wait', () => {
    expect(failureOf(apiError(429, 42))).toEqual({ kind: 'failed', reason: 'busy', retryAfter: 42, problems: [] });
  });

  it('falls back to the one minute cooldown when the server sends no wait', () => {
    expect(failureOf(apiError(429))).toEqual({ kind: 'failed', reason: 'busy', retryAfter: 60, problems: [] });
  });

  it('keeps the field positions of a validation failure', () => {
    const problems = [{ field: 'captures.0.status', message: 'must be a status code' }];
    expect(failureOf(apiError(422, null, problems))).toEqual({
      kind: 'failed',
      reason: 'validation',
      retryAfter: null,
      problems,
    });
  });

  it('tells a missing session apart from a missing role on 403', () => {
    expect(failureOf(apiError(403, null, [], 'session_required')).reason).toBe('sessionRequired');
    expect(failureOf(apiError(403, null, [], 'not_available_in_mode')).reason).toBe('sessionRequired');
    expect(failureOf(apiError(403, null, [], 'forbidden')).reason).toBe('forbidden');
  });

  it('maps a network failure and an unknown error to their own reasons', () => {
    expect(failureOf(new ImportNetworkError()).reason).toBe('network');
    expect(failureOf(new Error('unexpected')).reason).toBe('generic');
  });
});

describe('sendImport', () => {
  it('resolves with the counts of an import into an endpoint', async () => {
    const records: ImportRecord[] = [];
    mockServer.use(
      ...importHandlers({
        into: { status: 200, body: { imported: 3, skipped: 1, reason: 'capacity' } },
        onRequest: (record) => {
          records.push(record);
        },
      }),
    );

    const outcome = await sendImport(upload(ENDPOINT_ID, false));

    expect(outcome).toEqual({ imported: 3, skipped: 1, reason: 'capacity', endpointId: null });
    expect(records).toEqual([{ path: IMPORT_INTO_PATH, keepTimes: 'false', contentType: 'application/json' }]);
  });

  it('resolves with the id of the endpoint created by an import', async () => {
    mockServer.use(...importHandlers());

    const outcome = await sendImport(upload(null));

    expect(outcome.endpointId).toBe(ENDPOINT_ID);
    expect(outcome.imported).toBe(2);
  });

  it('rejects with the field positions the server sent', async () => {
    mockServer.use(
      ...importHandlers({
        into: {
          status: 422,
          body: {
            error: {
              code: 'validation_failed',
              message: 'The file has problems.',
              fields: [{ field: 'line.3', message: 'must be an object' }],
            },
          },
        },
      }),
    );

    const failure = await sendImport(upload(ENDPOINT_ID)).then(
      () => null,
      (error: unknown) => failureOf(error),
    );

    expect(failure).toEqual({
      kind: 'failed',
      reason: 'validation',
      retryAfter: null,
      problems: [{ field: 'line.3', message: 'must be an object' }],
    });
  });

  it('reads the Retry-After header of a busy answer', async () => {
    mockServer.use(
      http.post(IMPORT_INTO_PATH, () =>
        HttpResponse.json(
          { error: { code: 'rate_limited', message: 'Wait for the running import.' } },
          { status: 429, headers: { 'Retry-After': '30' } },
        ),
      ),
    );

    const failure = await sendImport(upload(ENDPOINT_ID)).then(
      () => null,
      (error: unknown) => failureOf(error),
    );

    expect(failure).toMatchObject({ reason: 'busy', retryAfter: 30 });
  });
});
