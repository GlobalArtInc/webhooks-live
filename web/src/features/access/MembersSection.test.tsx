import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { Member, User } from '../../api/types';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { mockStore, seedEndpoint, seedUser } from '../../test/msw/store';
import { renderAppAt } from '../endpoints/renderApp';
import { endpointWith, installAccessMock, seedClientInstance } from './accessMocks';

registerMockServer();

afterEach(() => {
  cleanup();
});

const OWNER: User = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21', email: 'owner@example.test', name: 'Olga Owner' };
const OPS: User = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a22', email: 'ops@example.test', name: 'Dev Ops' };
const NEWCOMER: User = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a23', email: 'new@example.test', name: null };
const OPS_MEMBER: Member = { user: OPS, role: 'viewer', added_at: '2026-11-21T10:00:00.000Z' };

function openMembers(myRole: 'owner' | 'editor' = 'owner') {
  seedClientInstance('sso');
  mockStore.instance = { ...mockStore.instance, user: OWNER };
  const endpoint = endpointWith({ owner: OWNER, my_role: myRole });
  seedEndpoint(endpoint);
  installAccessMock(endpoint);
  seedUser(OPS);
  seedUser(NEWCOMER);
  mockStore.members.set(ENDPOINT_ID, [OPS_MEMBER]);
  return renderAppAt(`/e/${ENDPOINT_ID}/settings/access`);
}

describe('members', () => {
  it('lists the owner and members and hides the viewing password for owned endpoints', async () => {
    openMembers();

    const list = await screen.findByRole('list', { name: 'Members' });
    await waitFor(() => expect(within(list).getByText('Dev Ops')).toBeTruthy());
    expect(within(list).getByText('Olga Owner (you)')).toBeTruthy();
    expect(screen.queryByText('Viewing password')).toBeNull();
  });

  it('changes a role only after the owner confirms', async () => {
    openMembers();

    const select = await screen.findByRole('combobox', { name: 'Role of Dev Ops' });
    fireEvent.change(select, { target: { value: 'editor' } });
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Change role' })).toBeNull());
    expect(mockStore.members.get(ENDPOINT_ID)?.[0]?.role).toBe('viewer');

    fireEvent.change(select, { target: { value: 'editor' } });
    fireEvent.click(await screen.findByRole('button', { name: 'Change role' }));
    await waitFor(() => expect(mockStore.members.get(ENDPOINT_ID)?.[0]?.role).toBe('editor'));
    await waitFor(() =>
      expect((screen.getByRole('combobox', { name: 'Role of Dev Ops' }) as HTMLSelectElement).value).toBe('editor'),
    );
  });

  it('removes a member after confirmation', async () => {
    openMembers();

    fireEvent.click(await screen.findByRole('button', { name: 'Remove Dev Ops' }));
    expect(await screen.findByText('Dev Ops will lose access to this endpoint.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));

    await waitFor(() => expect(mockStore.members.get(ENDPOINT_ID)).toEqual([]));
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Remove Dev Ops' })).toBeNull());
  });

  it('adds a person by email and explains an unknown email', async () => {
    openMembers();

    fireEvent.click(await screen.findByRole('button', { name: 'Invite' }));
    const email = await screen.findByLabelText('Email address');
    fireEvent.change(email, { target: { value: 'stranger@example.test' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add member' }));
    expect(await screen.findByText(/Nobody with this email has signed in yet/)).toBeTruthy();

    fireEvent.change(email, { target: { value: 'not-an-email' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add member' }));
    expect(await screen.findByText('Enter a valid email address.')).toBeTruthy();

    fireEvent.change(email, { target: { value: NEWCOMER.email } });
    fireEvent.change(screen.getByLabelText('Role'), { target: { value: 'editor' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add member' }));

    await waitFor(() => expect(screen.queryByLabelText('Email address')).toBeNull());
    expect(await screen.findByRole('combobox', { name: 'Role of new@example.test' })).toBeTruthy();
    expect(
      mockStore.members.get(ENDPOINT_ID)?.some((member) => member.user.id === NEWCOMER.id && member.role === 'editor'),
    ).toBe(true);
  });

  it('shows only the owner to someone who cannot manage members', async () => {
    openMembers('editor');

    expect(await screen.findByText('Only the owner can see and change the other members.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Invite' })).toBeNull();
    expect(screen.queryByRole('combobox', { name: 'Role of Dev Ops' })).toBeNull();
  });
});
