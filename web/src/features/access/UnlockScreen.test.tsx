import { cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { errorResponse, jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { renderAppAt } from '../endpoints/renderApp';
import { FAKE_GRANT, FAKE_UNLOCK_PASSWORD, installAccessMock, type AccessMockOptions } from './accessMocks';
import { clearGrant, readGrant } from './grants';

registerMockServer();

afterEach(() => {
  cleanup();
  clearGrant(ENDPOINT_ID);
});

const UNLOCK_PATH = `/e/${ENDPOINT_ID}/unlock`;
const WRONG_TEXT = 'That password did not work. Check it and try again.';

function openUnlock(options: AccessMockOptions = {}) {
  seedEndpoint(endpointFixture);
  const mock = installAccessMock(endpointFixture, options);
  return { mock, view: renderAppAt(UNLOCK_PATH) };
}

function openUnlockWithNext(next: string) {
  seedEndpoint(endpointFixture);
  const mock = installAccessMock(endpointFixture);
  return { mock, view: renderAppAt(`${UNLOCK_PATH}?next=${encodeURIComponent(next)}`) };
}

async function submitPassword(value: string): Promise<HTMLInputElement> {
  const input = await screen.findByLabelText('Password');
  fireEvent.change(input, { target: { value } });
  fireEvent.click(screen.getByRole('button', { name: 'Unlock' }));
  return input as HTMLInputElement;
}

describe('unlock page', () => {
  it('shows a neutral error for a wrong password, clears the field, and returns focus to it', async () => {
    const { mock } = openUnlock();

    const input = await submitPassword('wrong-guess');

    expect((await screen.findByRole('alert')).textContent).toBe(WRONG_TEXT);
    expect(input.value).toBe('');
    await waitFor(() => {
      expect(document.activeElement).toBe(input);
    });
    expect(input.getAttribute('autocomplete')).toBe('current-password');
    expect(input.getAttribute('type')).toBe('password');
    expect(mock.unlockAttempts).toEqual(['wrong-guess']);
    expect(readGrant(ENDPOINT_ID)).toBeNull();
  });

  it('answers a missing endpoint exactly like a wrong password', async () => {
    openUnlock({ unlockStatus: 410 });

    await submitPassword('wrong-guess');

    expect((await screen.findByRole('alert')).textContent).toBe(WRONG_TEXT);
  });

  it('blocks the form while rate limited and counts down from Retry-After', async () => {
    openUnlock({ unlockStatus: 429, retryAfter: '45' });

    await submitPassword('wrong-guess');

    expect(await screen.findByText('Too many attempts. Wait before trying again.')).toBeTruthy();
    expect(screen.getByText('You can try again in 45 s.')).toBeTruthy();
    expect((screen.getByRole('button', { name: 'Unlock' }) as HTMLButtonElement).disabled).toBe(true);
  });

  it('returns to a path that carries its own query string', async () => {
    const { view } = openUnlockWithNext(`/e/${ENDPOINT_ID}/settings/access?x=1`);

    await submitPassword(FAKE_UNLOCK_PASSWORD);

    await waitFor(() => expect(view.router.state.location.pathname).toBe(`/e/${ENDPOINT_ID}/settings/access`));
  });

  it('opens a locked endpoint after the right password and sends the grant afterwards', async () => {
    seedEndpoint(endpointFixture);
    const seenGrants: (string | null)[] = [];
    installAccessMock(endpointFixture);
    mockServer.use(
      http.get(`/api/v1/endpoints/${ENDPOINT_ID}`, ({ request }) => {
        const grant = request.headers.get('X-Webhooks-Live-Grant');
        seenGrants.push(grant);
        return grant === FAKE_GRANT
          ? jsonResponse(endpointFixture)
          : errorResponse(401, 'grant_required', 'This endpoint needs a password.');
      }),
    );
    const view = renderAppAt(`/e/${ENDPOINT_ID}`);

    expect(await screen.findByRole('heading', { name: 'This endpoint is locked' })).toBeTruthy();
    expect(view.router.state.location.pathname).toBe(UNLOCK_PATH);
    expect(view.router.state.location.search).toEqual({ next: `/e/${ENDPOINT_ID}` });

    await submitPassword(FAKE_UNLOCK_PASSWORD);

    await waitFor(() => expect(view.router.state.location.pathname).toBe(`/e/${ENDPOINT_ID}`));
    expect(readGrant(ENDPOINT_ID)).toBe(FAKE_GRANT);
    expect(seenGrants).toContain(FAKE_GRANT);
    expect(screen.queryByRole('heading', { name: 'This endpoint is locked' })).toBeNull();
    expect(JSON.stringify(view.router.state.location)).not.toContain(FAKE_UNLOCK_PASSWORD);
  });
});
