import { http, HttpResponse } from 'msw';
import { describe, expect, it, vi } from 'vitest';
import { captureFixtures } from '../test/fixtures/captures';
import { ENDPOINT_ID, endpointFixture } from '../test/fixtures/endpoint';
import { mockServer, registerMockServer } from '../test/msw/server';
import { seedEndpoint } from '../test/msw/store';
import { ApiError, getCapture, getEndpoint, getInstance, listCaptures, setRateLimitObserver } from './client';

registerMockServer();

const ENDPOINT_PATH = '/api/v1/endpoints/:endpointId';
const CAPTURES_PATH = '/api/v1/endpoints/:endpointId/captures';
const MISSING_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5fff';

describe('typed API client', () => {
  it('maps a 404 answer into an ApiError with the error code and message', async () => {
    seedEndpoint(endpointFixture, captureFixtures);

    await expect(getCapture(ENDPOINT_ID, MISSING_CAPTURE_ID)).rejects.toBeInstanceOf(ApiError);
    await expect(getCapture(ENDPOINT_ID, MISSING_CAPTURE_ID)).rejects.toMatchObject({
      status: 404,
      code: 'capture_not_found',
      details: [],
    });
  });

  it('maps a 410 answer without a JSON error body into an ApiError with a null code', async () => {
    mockServer.use(
      http.get(ENDPOINT_PATH, () =>
        HttpResponse.text('This endpoint does not exist or has expired.', {
          status: 410,
          headers: { 'Content-Type': 'text/plain' },
        }),
      ),
    );

    await expect(getEndpoint(ENDPOINT_ID)).rejects.toMatchObject({ status: 410, code: null });
  });

  it('maps a 429 answer into an ApiError carrying the Retry-After seconds', async () => {
    mockServer.use(
      http.get(ENDPOINT_PATH, () =>
        HttpResponse.json(
          { error: { code: 'rate_limited', message: 'Too many requests.' } },
          { status: 429, headers: { 'Retry-After': '7' } },
        ),
      ),
    );

    await expect(getEndpoint(ENDPOINT_ID)).rejects.toMatchObject({
      status: 429,
      code: 'rate_limited',
      retryAfter: 7,
    });
  });

  it('passes the RateLimit headers of an answer to the observer', async () => {
    const observer = vi.fn();
    setRateLimitObserver(observer);
    try {
      await getInstance();
    } finally {
      setRateLimitObserver(null);
    }

    expect(observer).toHaveBeenCalledWith({ limit: 60, remaining: 59 });
  });

  it('sends only the capture filters that are set', async () => {
    let requested = '';
    mockServer.use(
      http.get(CAPTURES_PATH, ({ request }) => {
        requested = request.url;
        return HttpResponse.json({ items: [], next_cursor: null, total: 0 });
      }),
    );

    await listCaptures(ENDPOINT_ID, { method: 'POST,PUT', limit: 2, cursor: null, q: undefined });

    expect(Object.fromEntries(new URL(requested).searchParams)).toEqual({ method: 'POST,PUT', limit: '2' });
  });
});
