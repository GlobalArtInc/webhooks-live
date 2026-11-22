import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { renderAppAt } from '../../features/endpoints/renderApp';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints } from '../../features/endpoints/storage';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe('a saved endpoint title', () => {
  it('replaces the id in the sidebar entry', async () => {
    seedEndpoint({ ...endpointFixture, title: null }, []);
    window.localStorage.setItem(
      ENDPOINTS_STORAGE_KEY,
      serializeEndpoints([{ id: ENDPOINT_ID, label: null, added: 1, lastOpened: 2, lastSeenCaptureId: null }]),
    );
    renderAppAt(`/e/${ENDPOINT_ID}/settings/general`);
    const nav = await screen.findByRole('navigation', { name: 'My endpoints' });
    expect(await within(nav).findByRole('link', { name: '7k2m9q...2p4' })).toBeTruthy();

    fireEvent.change(await screen.findByLabelText('Title'), { target: { value: 'My title' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(await within(nav).findByRole('link', { name: 'My title' })).toBeTruthy();
    await waitFor(() => expect(document.title).toBe('My title · Settings · General — webhooks.live'));
  });

  it('puts the title into the document title of the capture list', async () => {
    seedEndpoint({ ...endpointFixture, title: 'My title' }, []);
    renderAppAt(`/e/${ENDPOINT_ID}`);

    await waitFor(() => expect(document.title).toBe('My title — webhooks.live'));
  });
});
