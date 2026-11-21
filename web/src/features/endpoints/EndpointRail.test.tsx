import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { shortEndpointId } from './format';
import { renderAppAt } from './renderApp';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints, type MyEndpoint } from './storage';

registerMockServer();

const OTHER_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';

function seedTwo(): void {
  const entries: MyEndpoint[] = [
    { id: ENDPOINT_ID, label: 'stripe-dev', added: 1, lastOpened: 2, lastSeenCaptureId: null },
    { id: OTHER_ID, label: null, added: 1, lastOpened: 1, lastSeenCaptureId: null },
  ];
  window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, serializeEndpoints(entries));
}

async function findRail(): Promise<HTMLElement> {
  return screen.findByRole('navigation', { name: 'My endpoints' });
}

describe('EndpointRail', () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it('lists my endpoints and marks the open one as the current page', async () => {
    seedEndpoint(endpointFixture);
    seedTwo();
    renderAppAt(`/e/${ENDPOINT_ID}`);

    const nav = await findRail();
    const current = await within(nav).findByRole('link', { name: 'stripe-dev' });
    expect(current.getAttribute('aria-current')).toBe('page');
    expect(
      within(nav)
        .getByRole('link', { name: shortEndpointId(OTHER_ID) })
        .getAttribute('aria-current'),
    ).toBeNull();
  });

  it('collapses to the narrow rail from its toggle and expands it again', async () => {
    seedTwo();
    renderAppAt('/');
    const nav = await findRail();
    await within(nav).findByRole('link', { name: 'stripe-dev' });

    fireEvent.click(within(nav).getByRole('button', { name: 'Collapse' }));

    expect(within(nav).getByRole('button', { name: 'Expand' }).getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(within(nav).getByRole('button', { name: 'Expand' }));
    expect(within(nav).getByRole('button', { name: 'Collapse' }).getAttribute('aria-expanded')).toBe('true');
  });

  it('creates an endpoint from New and opens it', async () => {
    const { router } = renderAppAt('/');
    const nav = await findRail();

    fireEvent.click(within(nav).getByRole('button', { name: 'New endpoint' }));

    await waitFor(() => expect(router.state.location.pathname).toMatch(/^\/e\/[0-9a-z]{24}$/));
  });
});
