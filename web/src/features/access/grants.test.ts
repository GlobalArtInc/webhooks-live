import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import { getEndpoint, setGrantResolver } from '../../api/client';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { clearGrant, installGrantResolver, readGrant, storeGrant } from './grants';

registerMockServer();

const FUTURE = '2999-01-01T00:00:00.000Z';

afterEach(() => {
  clearGrant(ENDPOINT_ID);
  setGrantResolver(null);
});

describe('grants', () => {
  it('stores a grant for the session and returns it until it expires', () => {
    expect(storeGrant(ENDPOINT_ID, 'fake-grant', FUTURE)).toBe(true);
    expect(readGrant(ENDPOINT_ID)).toBe('fake-grant');
    expect(window.sessionStorage.getItem(`wl.grant.${ENDPOINT_ID}`)).toContain('fake-grant');
    expect(readGrant(ENDPOINT_ID, Date.parse(FUTURE) + 1)).toBeNull();
    expect(window.sessionStorage.getItem(`wl.grant.${ENDPOINT_ID}`)).toBeNull();
  });

  it('refuses an empty grant or an unreadable expiry', () => {
    expect(storeGrant(ENDPOINT_ID, '', FUTURE)).toBe(false);
    expect(storeGrant(ENDPOINT_ID, 'fake-grant', 'tomorrow-ish')).toBe(false);
    expect(readGrant(ENDPOINT_ID)).toBeNull();
  });

  it('sends the grant header on reads of that endpoint only', async () => {
    const seen: (string | null)[] = [];
    mockServer.use(
      http.get(`/api/v1/endpoints/${ENDPOINT_ID}`, ({ request }) => {
        seen.push(request.headers.get('X-Webhooks-Live-Grant'));
        return jsonResponse(endpointFixture);
      }),
    );
    installGrantResolver();

    await getEndpoint(ENDPOINT_ID);
    storeGrant(ENDPOINT_ID, 'fake-grant', FUTURE);
    await getEndpoint(ENDPOINT_ID);

    expect(seen).toEqual([null, 'fake-grant']);
  });
});
