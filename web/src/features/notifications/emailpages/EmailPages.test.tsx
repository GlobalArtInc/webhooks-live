import { cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { DISABLE_TOKEN, pendingAddressFixture, VERIFY_TOKEN, WRONG_TOKEN } from '../../../test/fixtures/notifications';
import { notificationHandlers, type NotificationApiState } from '../../../test/msw/notificationHandlers';
import { mockServer, registerMockServer } from '../../../test/msw/server';
import { renderAppAt } from '../../endpoints/renderApp';

registerMockServer();

afterEach(() => {
  cleanup();
  mockServer.events.removeAllListeners();
  window.history.replaceState(null, '', '/');
});

const VERIFY_BUTTON = 'Confirm this address';
const DISABLE_BUTTON = 'Stop these notifications';
const INVALID_VERIFY =
  'This link is invalid or has expired. Request a new confirmation from the notification settings.';

function serveState(): void {
  const state: NotificationApiState = {
    channels: [],
    addresses: [pendingAddressFixture],
    secrets: [],
    testStatus: 'succeeded',
    patches: [],
  };
  mockServer.use(...notificationHandlers(state));
}

function openAt(path: string): void {
  window.history.replaceState(null, '', path);
  renderAppAt(path);
}

describe('e-mail landing pages', () => {
  it('does not post the token on load and removes it from the address bar', async () => {
    const posted: string[] = [];
    mockServer.events.on('request:start', ({ request }) => {
      if (request.method === 'POST') {
        posted.push(new URL(request.url).pathname);
      }
    });
    serveState();
    openAt(`/email/verify?token=${VERIFY_TOKEN}`);

    expect(await screen.findByRole('button', { name: VERIFY_BUTTON })).toBeTruthy();
    await waitFor(() => {
      expect(window.location.search).toBe('');
    });
    expect(window.location.href).not.toContain(VERIFY_TOKEN);
    expect(posted).toEqual([]);
  });

  it('posts the token when the button is pressed and then confirms the address', async () => {
    const posted: string[] = [];
    mockServer.events.on('request:start', ({ request }) => {
      if (request.method === 'POST') {
        posted.push(new URL(request.url).pathname);
      }
    });
    serveState();
    openAt(`/email/verify?token=${VERIFY_TOKEN}`);

    fireEvent.click(await screen.findByRole('button', { name: VERIFY_BUTTON }));

    expect(
      await screen.findByText(`${pendingAddressFixture.address} is confirmed. You can close this page.`),
    ).toBeTruthy();
    expect(posted).toEqual(['/api/v1/email-addresses/verify']);
  });

  it('shows the invalid state for a link the server refuses', async () => {
    serveState();
    openAt(`/email/verify?token=${WRONG_TOKEN}`);

    fireEvent.click(await screen.findByRole('button', { name: VERIFY_BUTTON }));

    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(screen.getByText(INVALID_VERIFY)).toBeTruthy();
  });

  it('shows the invalid state without a button when the link has no token', async () => {
    serveState();
    openAt('/email/verify');

    expect(await screen.findByText(INVALID_VERIFY)).toBeTruthy();
    expect(screen.queryByRole('button', { name: VERIFY_BUTTON })).toBeNull();
  });

  it('stops the notifications of the channel from the disable link', async () => {
    serveState();
    openAt(`/notifications/disable?token=${DISABLE_TOKEN}`);

    fireEvent.click(await screen.findByRole('button', { name: DISABLE_BUTTON }));

    expect(await screen.findByText('These notifications are stopped. You can close this page.')).toBeTruthy();
  });
});
