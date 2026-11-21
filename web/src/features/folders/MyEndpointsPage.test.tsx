import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { User } from '../../api/types';
import { endpointFixture } from '../../test/fixtures/endpoint';
import { instanceFixture } from '../../test/fixtures/instance';
import { registerMockServer } from '../../test/msw/server';
import { mockStore, seedEndpoint } from '../../test/msw/store';
import { renderAppAt } from '../endpoints/renderApp';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints, type MyEndpoint } from '../endpoints/storage';
import { ORGANIZE_STORAGE_KEY, serializeOrganize } from './localOrganize';

registerMockServer();

const OWNER: User = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'dev@example.test', name: 'Dev' };
const ACCOUNT_ID = '0123456789abcdefghjkmnpq';
const LOCAL_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';
const ACCOUNT_FOLDER_ID = '5a1b2c3d-0000-4000-8000-000000000001';
const LOCAL_FOLDER_ID = 'local-receipts';

function signIn(): void {
  mockStore.instance = {
    ...instanceFixture,
    auth_mode: 'mixed',
    user: OWNER,
    auth: { provider_name: null, sign_in_available: true },
  };
}

function seedAccountEndpoint(): void {
  seedEndpoint({
    ...endpointFixture,
    id: ACCOUNT_ID,
    title: 'stripe-dev',
    tags: ['billing'],
    owner: OWNER,
    my_role: 'owner',
  });
  mockStore.folders = [
    {
      id: ACCOUNT_FOLDER_ID,
      name: 'Payments',
      position: 0,
      endpoint_ids: [ACCOUNT_ID],
      created_at: '2026-11-21T10:00:00.000Z',
    },
  ];
}

function seedBrowserEndpoint(): void {
  seedEndpoint({ ...endpointFixture, id: LOCAL_ID, title: null, tags: [], owner: null, my_role: null });
  const entries: MyEndpoint[] = [
    { id: LOCAL_ID, label: 'github-app', added: 1, lastOpened: 2, lastSeenCaptureId: null },
  ];
  window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, serializeEndpoints(entries));
  window.localStorage.setItem(
    ORGANIZE_STORAGE_KEY,
    serializeOrganize({
      folders: [{ id: LOCAL_FOLDER_ID, name: 'Receipts' }],
      placements: [{ id: LOCAL_ID, folderId: LOCAL_FOLDER_ID, tags: ['ci'] }],
    }),
  );
}

function cardOf(main: HTMLElement, name: string): HTMLElement | null {
  return within(main).getByRole('link', { name }).closest('li');
}

describe('MyEndpointsPage', () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it('lists an account endpoint and a browser endpoint once each, with their sources', async () => {
    signIn();
    seedAccountEndpoint();
    seedBrowserEndpoint();
    renderAppAt('/endpoints');

    const main = await screen.findByRole('main');
    const account = await within(main).findByRole('link', { name: 'stripe-dev' });

    expect(within(main).getAllByRole('link', { name: 'stripe-dev' })).toHaveLength(1);
    expect(within(main).getAllByRole('link', { name: 'github-app' })).toHaveLength(1);
    expect(cardOf(main, 'stripe-dev')?.textContent ?? '').toContain('Account');
    expect(cardOf(main, 'github-app')?.textContent ?? '').toContain('This browser');
    expect(account.getAttribute('href')).toBe(`/e/${ACCOUNT_ID}`);
  });

  it('moves a browser endpoint to the account and keeps its label and folder', async () => {
    signIn();
    seedBrowserEndpoint();
    renderAppAt('/endpoints');
    const main = await screen.findByRole('main');

    fireEvent.click(await within(main).findByRole('button', { name: 'Move github-app to account' }));

    await within(main).findByText('Moved to your account.');
    await waitFor(() => {
      expect(cardOf(main, 'github-app')?.textContent ?? '').toContain('Account');
      const folder = within(main).getByRole('combobox', { name: 'Folder for github-app' }) as HTMLSelectElement;
      expect(folder.selectedOptions[0]?.textContent).toBe('Receipts');
    });
    expect(within(main).getAllByRole('link', { name: 'github-app' })).toHaveLength(1);
  });
});
