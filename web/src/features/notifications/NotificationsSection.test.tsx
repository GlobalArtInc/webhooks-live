import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import type { Instance } from '../../api/types';
import { ENDPOINT_ID, endpointFixture } from '../../test/fixtures/endpoint';
import { instanceFixture } from '../../test/fixtures/instance';
import {
  emailChannelFixture,
  pendingAddressFixture,
  secretFixtures,
  slackChannelFixture,
  verifiedAddressFixture,
} from '../../test/fixtures/notifications';
import { notificationHandlers, type NotificationApiState } from '../../test/msw/notificationHandlers';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { renderAppAt } from '../endpoints/renderApp';

registerMockServer();

afterEach(() => {
  cleanup();
});

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/notifications`;
const INSTANCE_PATH = '/api/v1/instance';
const SIGNED_IN_USER = { id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4f01', email: 'alex@example.test', name: 'Alex' };

function enableNotifications(signedIn: boolean): void {
  const instance: Instance = {
    ...instanceFixture,
    user: signedIn ? SIGNED_IN_USER : null,
    features: { ...instanceFixture.features, notifications: ['webhook', 'slack', 'email', 'push'] },
  };
  mockServer.use(http.get(INSTANCE_PATH, () => jsonResponse(instance)));
}

function apiState(overrides: Partial<NotificationApiState> = {}): NotificationApiState {
  return {
    channels: [slackChannelFixture, emailChannelFixture],
    addresses: [verifiedAddressFixture, pendingAddressFixture],
    secrets: secretFixtures,
    testStatus: 'succeeded',
    patches: [],
    ...overrides,
  };
}

function serveState(state: NotificationApiState): void {
  mockServer.use(...notificationHandlers(state));
}

describe('notifications section', () => {
  it('explains that channels are off when the instance lists no channel kinds', async () => {
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    serveState(apiState());
    renderAppAt(SECTION_PATH);

    expect(await screen.findByText('Notifications are not available on this instance.')).toBeTruthy();
  });

  it('turns a channel off with its switch', async () => {
    enableNotifications(false);
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    const state = apiState();
    serveState(state);
    renderAppAt(SECTION_PATH);

    const toggle = await screen.findByRole('switch', { name: 'Send to ops-chat' });
    expect(toggle.getAttribute('aria-checked')).toBe('true');
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(state.patches).toEqual([{ enabled: false }]);
    });
  });

  it('sends no config when only the name of a channel with a masked URL changes', async () => {
    enableNotifications(false);
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    const state = apiState();
    serveState(state);
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Edit ops-chat' }));
    expect(
      await screen.findByText('Saved as https://hooks.slack.com/services/.... Type a new URL to replace it.'),
    ).toBeTruthy();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'ops-room' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => {
      expect(state.patches).toHaveLength(1);
    });
    expect(state.patches[0]).toEqual({ name: 'ops-room', per_hour: 60 });
    expect(state.patches[0]).not.toHaveProperty('config');
  });

  it('shows a successful test inline with its time', async () => {
    enableNotifications(false);
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    serveState(apiState());
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Send a test to ops-chat' }));

    expect(await screen.findByText(/^Test delivered at /)).toBeTruthy();
  });

  it('shows the status and error of a failed test inline', async () => {
    enableNotifications(false);
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    serveState(apiState({ testStatus: 'failed' }));
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Send a test to ops-chat' }));

    expect(await screen.findByText(/^Test failed at .*: 401 Unauthorized$/)).toBeTruthy();
  });

  it('says when a test is sent too often and when to try again', async () => {
    enableNotifications(false);
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    serveState(apiState({ testStatus: 'rate-limited' }));
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Send a test to ops-chat' }));

    expect(await screen.findByText(/^Test not sent at .*\. Too many requests\. Try again in 30 s\.$/)).toBeTruthy();
  });

  it('lists confirmed and waiting addresses for a signed-in user', async () => {
    enableNotifications(true);
    seedEndpoint({ ...endpointFixture, my_role: 'owner' }, []);
    serveState(apiState());
    renderAppAt(SECTION_PATH);

    const addresses = await screen.findByRole('list', { name: 'E-mail addresses' });
    expect(within(addresses).getByText('alex@mail.example.test')).toBeTruthy();
    expect(within(addresses).getByText('Confirmed')).toBeTruthy();
    expect(within(addresses).getByText('Waiting for confirmation')).toBeTruthy();
  });
});
