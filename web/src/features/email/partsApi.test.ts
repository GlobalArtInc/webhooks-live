import { http, HttpResponse } from 'msw';
import { describe, expect, it } from 'vitest';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { errorResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { fetchPartBlob, partHref, PartRequestError } from './partsApi';
import { EMAIL_CAPTURE_ID } from './testFixtures';

registerMockServer();

const PART_ROUTE = `/api/v1/endpoints/${ENDPOINT_ID}/captures/${EMAIL_CAPTURE_ID}/parts/:index`;

describe('partHref', () => {
  it('builds the parts route with the download flag', () => {
    expect(partHref(ENDPOINT_ID, EMAIL_CAPTURE_ID, 3)).toBe(
      `/api/v1/endpoints/${ENDPOINT_ID}/captures/${EMAIL_CAPTURE_ID}/parts/3`,
    );
    expect(partHref(ENDPOINT_ID, EMAIL_CAPTURE_ID, 3, { download: true })).toMatch(/\/parts\/3\?download=1$/);
  });

  it('encodes ids and refuses an index that is not a plain non-negative integer', () => {
    expect(partHref('a/b', 'c?d', 0)).toContain('/endpoints/a%2Fb/captures/c%3Fd/parts/0');
    expect(() => partHref(ENDPOINT_ID, EMAIL_CAPTURE_ID, -1)).toThrow(PartRequestError);
    expect(() => partHref(ENDPOINT_ID, EMAIL_CAPTURE_ID, 1.5)).toThrow(PartRequestError);
    expect(() => partHref(ENDPOINT_ID, EMAIL_CAPTURE_ID, Number.NaN)).toThrow(PartRequestError);
  });
});

describe('fetchPartBlob', () => {
  it('returns the part content and asks for the text view when told to', async () => {
    const seen: string[] = [];
    mockServer.use(
      http.get(PART_ROUTE, ({ request }) => {
        seen.push(new URL(request.url).search);
        return new HttpResponse('hello', { headers: { 'Content-Type': 'text/plain' } });
      }),
    );

    const blob = await fetchPartBlob(ENDPOINT_ID, EMAIL_CAPTURE_ID, 0, { view: 'text', maxBytes: 100 });

    expect(await blob.text()).toBe('hello');
    expect(seen).toEqual(['?view=text']);
  });

  it('refuses a part larger than the limit', async () => {
    mockServer.use(http.get(PART_ROUTE, () => new HttpResponse('x'.repeat(50))));

    await expect(fetchPartBlob(ENDPOINT_ID, EMAIL_CAPTURE_ID, 0, { maxBytes: 10 })).rejects.toThrow(PartRequestError);
  });

  it('stops when the caller aborts', async () => {
    mockServer.use(http.get(PART_ROUTE, () => new HttpResponse('hello')));
    const controller = new AbortController();
    controller.abort();

    await expect(
      fetchPartBlob(ENDPOINT_ID, EMAIL_CAPTURE_ID, 0, { maxBytes: 100, signal: controller.signal }),
    ).rejects.toThrow();
  });

  it('reports the status of a failed request', async () => {
    mockServer.use(http.get(PART_ROUTE, () => errorResponse(404, 'part_not_found', 'No such part.')));

    await expect(fetchPartBlob(ENDPOINT_ID, EMAIL_CAPTURE_ID, 9, { maxBytes: 10 })).rejects.toMatchObject({
      status: 404,
    });
  });
});
