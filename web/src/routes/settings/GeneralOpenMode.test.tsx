import { cleanup, screen, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import { renderAppAt } from '../../features/endpoints/renderApp';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { instanceFixture } from '../../test/fixtures/instance';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { mockStore, seedEndpoint } from '../../test/msw/store';

registerMockServer();

afterEach(() => {
  cleanup();
});

describe('General settings on an instance without sign-in', () => {
  it('says what is not available, asks for nothing it cannot get, and keeps the owner hints away', async () => {
    const refused: string[] = [];
    mockStore.instance = {
      ...instanceFixture,
      auth_mode: 'open',
      user: null,
      auth: { provider_name: null, sign_in_available: false },
      limits: { ...instanceFixture.limits, aliases: 0 },
    };
    seedEndpoint({ ...endpointFixture, owner: null, my_role: null }, []);
    mockServer.use(
      http.get('/api/v1/usage', ({ request }) => {
        refused.push(request.url);
        return jsonResponse({ error: { code: 'unauthenticated', message: 'sign in' } }, 401);
      }),
      http.get(`/api/v1/endpoints/${ENDPOINT_ID}/aliases`, ({ request }) => {
        refused.push(request.url);
        return jsonResponse({ error: { code: 'session_required', message: 'session only' } }, 403);
      }),
    );
    renderAppAt(`/e/${ENDPOINT_ID}/settings/general`);

    const aliases = (await screen.findByRole('heading', { name: 'Aliases' })).closest('section');
    const usage = screen.getByRole('heading', { name: 'Usage' }).closest('section');
    if (aliases === null || usage === null) {
      throw new Error('the General sections are not rendered');
    }
    expect(within(aliases).getByText('Not available on this instance.')).toBeTruthy();
    expect(within(usage).getByText('Not available on this instance.')).toBeTruthy();
    expect(screen.queryByText('Aliases could not be loaded.')).toBeNull();
    expect(screen.queryByText('Usage could not be loaded.')).toBeNull();
    expect(screen.queryByText('Sign in to file this endpoint in a folder.')).toBeNull();
    expect(screen.queryByText('Only the owner can change the lifetime and the capture limits.')).toBeNull();
    expect(screen.getByRole('radio', { name: 'Keep until deleted' }).hasAttribute('disabled')).toBe(false);
    expect(refused).toEqual([]);
  });

  it('keeps the sign-in hint where signing in is possible', async () => {
    mockStore.instance = {
      ...instanceFixture,
      auth_mode: 'mixed',
      user: null,
      auth: { provider_name: 'Example SSO', sign_in_available: true },
    };
    seedEndpoint({ ...endpointFixture, owner: null, my_role: null }, []);
    renderAppAt(`/e/${ENDPOINT_ID}/settings/general`);

    expect(await screen.findByText('Sign in to file this endpoint in a folder.')).toBeTruthy();
  });
});
