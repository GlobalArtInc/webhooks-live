import { cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { renderAppAt } from '../endpoints/renderApp';
import {
  FAKE_GRANT,
  FAKE_NEW_PASSWORD,
  FAKE_PATCH_GRANT,
  FAKE_UNLOCK_PASSWORD,
  endpointWith,
  installAccessMock,
  seedClientInstance,
  type AccessMock,
  type AccessMockOptions,
} from './accessMocks';
import { clearGrant, readGrant, storeGrant } from './grants';

registerMockServer();

afterEach(() => {
  cleanup();
  clearGrant(ENDPOINT_ID);
  window.sessionStorage.clear();
  window.localStorage.clear();
});

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/access`;

function openSection(fields: Record<string, unknown> = {}, options: AccessMockOptions = {}) {
  const endpoint = endpointWith(fields);
  seedEndpoint(endpoint);
  const mock = installAccessMock(endpoint, options);
  return { mock, view: renderAppAt(SECTION_PATH) };
}

function storageDump(storage: Storage): string {
  return Array.from({ length: storage.length }, (_, index) => {
    const key = storage.key(index) ?? '';
    return `${key}=${storage.getItem(key) ?? ''}`;
  }).join('\n');
}

function save(): void {
  fireEvent.click(screen.getByRole('button', { name: 'Save' }));
}

async function chooseRadio(name: string): Promise<void> {
  fireEvent.click(await screen.findByRole('radio', { name }));
}

function lastPatch(mock: AccessMock) {
  return mock.patches.at(-1);
}

describe('viewing password', () => {
  it('asks twice, sends the password once, and keeps it out of caches, storage, and the URL', async () => {
    const { mock, view } = openSection();

    fireEvent.click(await screen.findByRole('button', { name: 'Set a password' }));
    const first = screen.getByLabelText('New password');
    expect(first.getAttribute('type')).toBe('password');
    expect(first.getAttribute('autocomplete')).toBe('new-password');
    expect(screen.getByLabelText('Type it again').getAttribute('autocomplete')).toBe('new-password');
    fireEvent.change(first, { target: { value: FAKE_NEW_PASSWORD } });
    fireEvent.change(screen.getByLabelText('Type it again'), { target: { value: FAKE_NEW_PASSWORD } });
    save();

    await waitFor(() => expect(mock.patches).toEqual([{ view_password: FAKE_NEW_PASSWORD }]));
    expect(await screen.findByText('A password is set. It cannot be shown again.')).toBeTruthy();
    expect(screen.queryByLabelText('New password')).toBeNull();
    const cached = JSON.stringify([
      ...view.queryClient
        .getQueryCache()
        .getAll()
        .map((query) => query.state.data),
      ...view.queryClient
        .getMutationCache()
        .getAll()
        .map((mutation) => [mutation.state.variables, mutation.state.data]),
    ]);
    expect(cached).not.toContain(FAKE_NEW_PASSWORD);
    expect(storageDump(window.localStorage)).not.toContain(FAKE_NEW_PASSWORD);
    expect(storageDump(window.sessionStorage)).not.toContain(FAKE_NEW_PASSWORD);
    expect(view.router.state.location.href).not.toContain(FAKE_NEW_PASSWORD);
  });

  it('refuses a mismatch without calling the server', async () => {
    const { mock } = openSection();

    fireEvent.click(await screen.findByRole('button', { name: 'Set a password' }));
    fireEvent.change(screen.getByLabelText('New password'), { target: { value: FAKE_NEW_PASSWORD } });
    fireEvent.change(screen.getByLabelText('Type it again'), { target: { value: 'something-else-1' } });
    save();

    expect(await screen.findByText('The two entries do not match.')).toBeTruthy();
    expect(mock.patches).toEqual([]);
  });

  it('removes a set password with null', async () => {
    const { mock } = openSection({ view_protected: true });

    fireEvent.click(await screen.findByRole('button', { name: 'Remove password' }));
    expect(screen.getByText('The password will be removed when you save.')).toBeTruthy();
    save();

    await waitFor(() => expect(mock.patches).toEqual([{ view_password: null }]));
    expect(
      await screen.findByText('No password is set. Anyone with the endpoint address can read its captures.'),
    ).toBeTruthy();
  });
});

describe('sender allow list', () => {
  it('rejects invalid ranges, normalizes valid ones, and saves them', async () => {
    const { mock } = openSection();

    fireEvent.click(await screen.findByRole('checkbox', { name: 'Accept captures only from these addresses' }));
    save();
    expect(await screen.findByText('Add at least one address or range, or turn the list off.')).toBeTruthy();
    expect(mock.patches).toEqual([]);

    const input = screen.getByRole('textbox', { name: 'Allowed addresses and ranges' });
    fireEvent.change(input, { target: { value: '203.0.113.5/24' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(await screen.findByText(/bits set after the prefix.*203\.0\.113\.0\/24/)).toBeTruthy();
    fireEvent.change(input, { target: { value: 'not-an-address' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(await screen.findByText(/Enter an IPv4 or IPv6 address/)).toBeTruthy();

    fireEvent.change(input, { target: { value: '203.0.113.0/24' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    fireEvent.change(input, { target: { value: '198.51.100.9' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    save();

    await waitFor(() => expect(lastPatch(mock)).toEqual({ sender_allow_cidrs: ['203.0.113.0/24', '198.51.100.9/32'] }));
  });

  it('offers to add the viewer address when it falls outside the list', async () => {
    seedClientInstance();
    const { mock } = openSection({ sender_allow_cidrs: ['203.0.113.0/24'] });

    expect(await screen.findByText('Your address 198.51.100.4 is outside the list.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Add it' }));
    save();

    await waitFor(() => expect(lastPatch(mock)).toEqual({ sender_allow_cidrs: ['203.0.113.0/24', '198.51.100.4/32'] }));
    await waitFor(() => expect(screen.queryByText(/is outside the list/)).toBeNull());
  });
});

describe('capture authentication', () => {
  it('generates a secret shown once, sends it, and never shows it again', async () => {
    const { mock } = openSection();

    await chooseRadio('Basic (user name and password)');
    fireEvent.change(screen.getByLabelText('User name'), { target: { value: 'sender' } });
    fireEvent.click(screen.getByRole('button', { name: 'Generate' }));
    const secret = screen.getByLabelText('Password');
    expect(document.activeElement).toBe(secret);
    expect(screen.getByText(/shown only once/)).toBeTruthy();
    const value = (secret as HTMLInputElement).value;
    expect(value).toHaveLength(32);
    save();

    await waitFor(() =>
      expect(lastPatch(mock)).toEqual({ capture_auth: { kind: 'basic', username: 'sender', password: value } }),
    );
    expect(await screen.findByText('Basic authentication is required for the user sender.')).toBeTruthy();
    expect((screen.getByLabelText('Password') as HTMLInputElement).value).toBe('');
    expect(screen.queryByText(/shown only once/)).toBeNull();
    expect(JSON.stringify(mock.current)).not.toContain(value);
  });

  it('asks for the secret again when the name changes and removes the requirement with null', async () => {
    const { mock } = openSection({ capture_auth: { kind: 'header', name: 'X-Test-Token' } });

    const name = await screen.findByLabelText('Header name');
    fireEvent.change(name, { target: { value: 'X-Other-Token' } });
    save();
    expect(await screen.findByText('Enter the secret, or generate one.')).toBeTruthy();
    expect(mock.patches).toEqual([]);

    await chooseRadio('None');
    save();
    await waitFor(() => expect(lastPatch(mock)).toEqual({ capture_auth: null }));
  });
});

describe('role gating', () => {
  it('shows everything read-only to a viewer', async () => {
    openSection({ my_role: 'viewer', sender_allow_cidrs: ['203.0.113.0/24'], view_protected: true });

    expect(await screen.findByText('Your role does not allow changes to access settings; ask the owner.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Change password' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
    expect(screen.getByText('203.0.113.0/24')).toBeTruthy();
    expect(screen.queryByRole('button', { name: /^Remove 203/ })).toBeNull();
  });
});

describe('grants and failed saves', () => {
  const FUTURE = '2999-01-01T00:00:00.000Z';

  it('stores the grant that the save answer carries and does not unlock again', async () => {
    storeGrant(ENDPOINT_ID, 'stale-grant', FUTURE);
    const { mock, view } = openSection({ view_protected: true });

    fireEvent.click(await screen.findByRole('button', { name: 'Change password' }));
    fireEvent.change(screen.getByLabelText('New password'), { target: { value: FAKE_NEW_PASSWORD } });
    fireEvent.change(screen.getByLabelText('Type it again'), { target: { value: FAKE_NEW_PASSWORD } });
    save();

    await waitFor(() => expect(readGrant(ENDPOINT_ID)).toBe(FAKE_PATCH_GRANT));
    expect(mock.unlockAttempts).toEqual([]);
    expect(JSON.stringify(view.queryClient.getQueryData(['endpoint', ENDPOINT_ID]))).not.toContain(FAKE_PATCH_GRANT);
  });

  it('unlocks with the new password when the save answer carries no grant', async () => {
    storeGrant(ENDPOINT_ID, 'stale-grant', FUTURE);
    const { mock } = openSection({ view_protected: true }, { omitViewGrant: true });

    fireEvent.click(await screen.findByRole('button', { name: 'Change password' }));
    fireEvent.change(screen.getByLabelText('New password'), { target: { value: FAKE_UNLOCK_PASSWORD } });
    fireEvent.change(screen.getByLabelText('Type it again'), { target: { value: FAKE_UNLOCK_PASSWORD } });
    save();

    await waitFor(() => expect(readGrant(ENDPOINT_ID)).toBe(FAKE_GRANT));
    expect(mock.unlockAttempts).toEqual([FAKE_UNLOCK_PASSWORD]);
  });

  it('drops the stored grant when the password is removed', async () => {
    storeGrant(ENDPOINT_ID, 'stale-grant', FUTURE);
    const { mock } = openSection({ view_protected: true });

    fireEvent.click(await screen.findByRole('button', { name: 'Remove password' }));
    save();

    await waitFor(() => expect(mock.patches).toEqual([{ view_password: null }]));
    await waitFor(() => expect(readGrant(ENDPOINT_ID)).toBeNull());
    expect(mock.unlockAttempts).toEqual([]);
  });

  it('keeps the typed secret and says the outcome is unknown after a server failure', async () => {
    openSection({}, { patchStatus: 503 });

    await chooseRadio('Secret header');
    fireEvent.change(screen.getByLabelText('Header name'), { target: { value: 'X-Test-Token' } });
    fireEvent.click(screen.getByRole('button', { name: 'Generate' }));
    const value = (screen.getByLabelText('Header value') as HTMLInputElement).value;
    save();

    expect(await screen.findByText(/not known whether the changes were saved/)).toBeTruthy();
    expect((screen.getByLabelText('Header value') as HTMLInputElement).value).toBe(value);
  });

  it('wipes the typed secret after a definitive refusal', async () => {
    openSection({}, { patchStatus: 422 });

    await chooseRadio('Secret header');
    fireEvent.change(screen.getByLabelText('Header name'), { target: { value: 'X-Test-Token' } });
    fireEvent.click(screen.getByRole('button', { name: 'Generate' }));
    save();

    await waitFor(() => expect((screen.getByLabelText('Header value') as HTMLInputElement).value).toBe(''));
  });

  it('hides a revealed password again once it is saved', async () => {
    openSection();

    fireEvent.click(await screen.findByRole('button', { name: 'Set a password' }));
    fireEvent.click(screen.getByRole('button', { name: 'Show' }));
    expect(screen.getByLabelText('New password').getAttribute('type')).toBe('text');
    fireEvent.change(screen.getByLabelText('New password'), { target: { value: FAKE_NEW_PASSWORD } });
    fireEvent.change(screen.getByLabelText('Type it again'), { target: { value: FAKE_NEW_PASSWORD } });
    save();

    fireEvent.click(await screen.findByRole('button', { name: 'Change password' }));
    expect(screen.getByLabelText('New password').getAttribute('type')).toBe('password');
  });

  it('does not offer to add the viewer address once the list is full', async () => {
    seedClientInstance();
    const full = Array.from({ length: 20 }, (_, index) => `203.0.${index}.0/24`);
    openSection({ sender_allow_cidrs: full });

    const add = await screen.findByRole('button', { name: 'Add it' });
    expect((add as HTMLButtonElement).disabled).toBe(true);
  });
});
