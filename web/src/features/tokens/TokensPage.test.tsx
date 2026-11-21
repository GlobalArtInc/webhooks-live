import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import { errorResponse } from '../../test/msw/responses';
import { renderAppAt } from '../endpoints/renderApp';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { mockStore } from '../../test/msw/store';
import { instanceFixture } from '../../test/fixtures/instance';
import { SIGNED_IN_INSTANCE, tokenFixture } from './testFixtures';
import { newTokenStore, tokenHandlers } from './testHandlers';

registerMockServer();

afterEach(() => {
  cleanup();
});

const ACTIVE = tokenFixture();
const EXPIRED = tokenFixture({
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a32',
  name: 'old-script',
  scopes: ['endpoints:write'],
  expires_at: '2026-01-01T00:00:00.000Z',
});
const REVOKED = tokenFixture({
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a33',
  name: 'laptop',
  endpoint_ids: ['7k2m9q4xw8c1d5f3h6j0n2p4', 'abcdefghjkmnpqrstvwxyz01'],
  last_used_at: '2026-11-20T10:00:00.000Z',
  revoked_at: '2026-11-20T11:00:00.000Z',
});

function signIn(): void {
  mockStore.instance = SIGNED_IN_INSTANCE;
}

describe('tokens page', () => {
  it('lists active, expired and revoked tokens with their state in text and offers Revoke only for active ones', async () => {
    signIn();
    mockServer.use(...tokenHandlers(newTokenStore([ACTIVE, EXPIRED, REVOKED])));
    renderAppAt('/settings/tokens');

    const table = await screen.findByRole('table', { name: 'API tokens' });
    const rows = within(table).getAllByRole('row');

    expect(rows).toHaveLength(4);
    expect(within(rows[1] as HTMLElement).getByText('Active')).toBeTruthy();
    expect(within(rows[1] as HTMLElement).getByText('Read')).toBeTruthy();
    expect(within(rows[1] as HTMLElement).getByText('Rules write')).toBeTruthy();
    expect(within(rows[1] as HTMLElement).getByText('Never used')).toBeTruthy();
    expect(within(rows[2] as HTMLElement).getByText('Expired')).toBeTruthy();
    expect(within(rows[3] as HTMLElement).getByText('Revoked')).toBeTruthy();
    expect(within(rows[3] as HTMLElement).getByText('2 endpoints')).toBeTruthy();
    expect(screen.getAllByRole('button', { name: /^Revoke / })).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'Revoke ci-runner' })).toBeTruthy();
  });

  it('revokes a token only after the confirmation and reports it', async () => {
    signIn();
    const store = newTokenStore([ACTIVE]);
    mockServer.use(...tokenHandlers(store));
    renderAppAt('/settings/tokens');

    fireEvent.click(await screen.findByRole('button', { name: 'Revoke ci-runner' }));
    const dialog = await screen.findByRole('dialog', { name: 'Revoke ci-runner?' });
    expect(store.revoked).toEqual([]);
    fireEvent.click(within(dialog).getByRole('button', { name: 'Revoke' }));

    expect(await screen.findByText('Token ci-runner was revoked.')).toBeTruthy();
    expect(store.revoked).toEqual([ACTIVE.id]);
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Revoke ci-runner' })).toBeNull());
    expect(screen.getByText('Revoked')).toBeTruthy();
  });

  it('does not revoke when the confirmation is cancelled', async () => {
    signIn();
    const store = newTokenStore([ACTIVE]);
    mockServer.use(...tokenHandlers(store));
    renderAppAt('/settings/tokens');

    fireEvent.click(await screen.findByRole('button', { name: 'Revoke ci-runner' }));
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(store.revoked).toEqual([]);
  });

  it('shows the empty state and opens the create dialog from the header button', async () => {
    signIn();
    mockServer.use(...tokenHandlers(newTokenStore()));
    renderAppAt('/settings/tokens');

    expect(await screen.findByText('No API tokens yet')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Create token' }));

    expect(await screen.findByRole('dialog', { name: 'Create token' })).toBeTruthy();
  });

  it('shows the not-available page when the instance has no sign-in', async () => {
    mockStore.instance = instanceFixture;
    renderAppAt('/settings/tokens');

    expect(await screen.findByRole('heading', { level: 1, name: 'API tokens are not available here' })).toBeTruthy();
    expect(screen.queryByRole('link', { name: 'API tokens' })).toBeNull();
  });

  it('asks an anonymous visitor on a mixed instance to sign in', async () => {
    mockStore.instance = { ...SIGNED_IN_INSTANCE, user: null, auth: { provider_name: 'Example ID', sign_in_available: true } };
    renderAppAt('/settings/tokens');

    const link = await screen.findByRole('link', { name: 'Sign in with Example ID' });

    expect(link.getAttribute('href')).toBe('/auth/login?return_to=%2Fsettings%2Ftokens');
    expect(screen.queryByRole('link', { name: 'API tokens' })).toBeNull();
  });

  it('links to the page from the header for a signed-in user', async () => {
    signIn();
    mockServer.use(...tokenHandlers(newTokenStore()));
    renderAppAt('/settings');

    const link = await screen.findByRole('link', { name: 'API tokens' });

    expect(link.getAttribute('href')).toBe('/settings/tokens');
  });

  it('refreshes the list and warns when the answer to a create request is lost after the server stored the token', async () => {
    signIn();
    const store = newTokenStore([ACTIVE]);
    store.commitThenFail = true;
    mockServer.use(...tokenHandlers(store));
    renderAppAt('/settings/tokens');
    fireEvent.click(await screen.findByRole('button', { name: 'Create token' }));
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'lost-answer' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(await screen.findByText(/the token may have been created/)).toBeTruthy();
    await waitFor(() => expect(screen.getAllByText('lost-answer').length).toBeGreaterThan(0));
    fireEvent.click(screen.getByRole('button', { name: 'Check the token list' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(within(await screen.findByRole('table', { name: 'API tokens' })).getByText('lost-answer')).toBeTruthy();
    expect(store.created).toHaveLength(1);
  });

  it('refreshes the row when the token was already revoked elsewhere', async () => {
    signIn();
    const store = newTokenStore([ACTIVE]);
    mockServer.use(...tokenHandlers(store));
    mockServer.use(
      http.delete('/api/v1/tokens/:tokenId', () => {
        store.tokens = [{ ...ACTIVE, revoked_at: '2026-11-21T09:00:00.000Z' }];
        return errorResponse(404, 'token_not_found', 'Token not found.');
      }),
    );
    renderAppAt('/settings/tokens');

    fireEvent.click(await screen.findByRole('button', { name: 'Revoke ci-runner' }));
    fireEvent.click(within(await screen.findByRole('dialog')).getByRole('button', { name: 'Revoke' }));

    expect(await screen.findByText('The token could not be revoked. Try again.')).toBeTruthy();
    expect(await screen.findByText('Revoked')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Revoke ci-runner' })).toBeNull();
  });
});
