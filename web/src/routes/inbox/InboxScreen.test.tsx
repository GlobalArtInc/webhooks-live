import { cleanup, screen, waitFor, within } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';
import { renderAppAt } from '../../features/endpoints/renderApp';
import {
  inboxCaptures,
  PAYMENTS_ENDPOINT_ID,
  PAYMENTS_TITLE,
  paymentsEndpoint,
  SHIPPING_ENDPOINT_ID,
  SHIPPING_TITLE,
  shippingEndpoint,
} from '../../test/fixtures/inbox';
import { inboxHandlers } from '../../test/msw/inbox';
import { mockServer, registerMockServer } from '../../test/msw/server';

registerMockServer();

const CHOSEN_SCOPE = `/inbox?scope=endpoints:${PAYMENTS_ENDPOINT_ID},${SHIPPING_ENDPOINT_ID}`;
const GONE_TITLE = 'An endpoint in this scope is no longer available';
const ROW_COUNT = 3;
const CLIENT_HEIGHT_PX = 360;

function listbox(): HTMLElement {
  return screen.getByRole('listbox', { name: 'Captures' });
}

function deleteButtons(): HTMLElement[] {
  return screen.getAllByRole('button', { name: 'Delete capture' });
}

describe('InboxScreen', () => {
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get: () => CLIENT_HEIGHT_PX,
    });
  });

  afterAll(() => {
    Reflect.deleteProperty(HTMLElement.prototype, 'clientHeight');
  });

  afterEach(() => {
    cleanup();
  });

  it('lists captures from the chosen endpoints with an endpoint chip on each row', async () => {
    mockServer.use(...inboxHandlers({ captures: inboxCaptures, endpoints: [paymentsEndpoint, shippingEndpoint] }));
    renderAppAt(CHOSEN_SCOPE);
    await waitFor(() => expect(within(listbox()).getAllByRole('option')).toHaveLength(ROW_COUNT));
    await waitFor(() => expect(within(listbox()).getAllByText(PAYMENTS_TITLE)).toHaveLength(2));
    expect(within(listbox()).getAllByText(SHIPPING_TITLE)).toHaveLength(1);
  });

  it('disables delete on rows whose endpoint the signed-in role cannot change', async () => {
    mockServer.use(...inboxHandlers({ captures: inboxCaptures, endpoints: [paymentsEndpoint, shippingEndpoint] }));
    renderAppAt(CHOSEN_SCOPE);
    await waitFor(() => expect(deleteButtons()).toHaveLength(ROW_COUNT));
    await waitFor(() => {
      const disabled = deleteButtons().filter((button) => button.hasAttribute('disabled'));
      expect(disabled).toHaveLength(1);
    });
  });

  it('asks the user to choose the scope again when an endpoint in it is gone', async () => {
    mockServer.use(...inboxHandlers({ captures: [], endpoints: [], listStatus: 410 }));
    renderAppAt(CHOSEN_SCOPE);
    const alert = await screen.findByRole('alert');
    expect(alert.textContent).toContain(GONE_TITLE);
  });
});
