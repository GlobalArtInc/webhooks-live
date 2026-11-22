import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { errorResponse, jsonResponse } from '../../test/msw/responses';
import { instanceFixture } from '../../test/fixtures/instance';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { mockStore } from '../../test/msw/store';
import { renderAppAt } from '../../features/endpoints/renderApp';
import {
  ENDPOINTS_STORAGE_KEY,
  parseEndpoints,
  serializeEndpoints,
  type MyEndpoint,
} from '../../features/endpoints/storage';
import { shortEndpointId } from '../../features/endpoints/format';

registerMockServer();

const ENDPOINTS_PATH = '/api/v1/endpoints';
const OPENED_PATH = /^\/e\/[0-9a-z]{24}$/;
const SHORT_ID = shortEndpointId(ENDPOINT_ID);

function seedOne(label: string | null = null): void {
  const entry: MyEndpoint = { id: ENDPOINT_ID, label, added: 1, lastOpened: Date.now(), lastSeenCaptureId: null };
  window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, serializeEndpoints([entry]));
}

async function openHome() {
  const view = renderAppAt('/');
  await screen.findByRole('heading', { level: 1, name: 'Watch HTTP requests arrive live.' });
  return view;
}

function listOf(): HTMLElement {
  return screen.getByRole('main');
}

describe('home screen', () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
    vi.restoreAllMocks();
  });

  it('creates an endpoint with the default response on click, opens it, and remembers it', async () => {
    const { router } = await openHome();

    fireEvent.click(screen.getByRole('button', { name: 'Create endpoint' }));

    await waitFor(() => expect(router.state.location.pathname).toMatch(OPENED_PATH));
    const opened = router.state.location.pathname.slice('/e/'.length);
    expect(parseEndpoints(window.localStorage.getItem(ENDPOINTS_STORAGE_KEY)).map((entry) => entry.id)).toEqual([
      opened,
    ]);
  });

  it('offers the import when the instance accepts uploads and hides it when its limit is zero', async () => {
    mockStore.instance = { ...instanceFixture, limits: { ...instanceFixture.limits, import_max_bytes: 104857600 } };
    const first = await openHome();
    expect(await screen.findByRole('button', { name: 'Import captures' })).toBeTruthy();
    first.unmount();

    mockStore.instance = { ...instanceFixture, limits: { ...instanceFixture.limits, import_max_bytes: 0 } };
    await openHome();
    await waitFor(() => expect(screen.getByRole('button', { name: 'Create endpoint' })).toBeTruthy());
    expect(screen.queryByRole('button', { name: 'Import captures' })).toBeNull();
  });

  it('shows a failure with Retry on a server error and creates the endpoint when retried', async () => {
    const { router } = await openHome();
    mockServer.use(http.post(ENDPOINTS_PATH, () => errorResponse(500, 'internal_error', 'boom')));

    fireEvent.click(screen.getByRole('button', { name: 'Create endpoint' }));

    expect(await screen.findByText('The server could not create the endpoint. Try again.')).toBeTruthy();
    expect(router.state.location.pathname).toBe('/');
    mockServer.resetHandlers();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(router.state.location.pathname).toMatch(OPENED_PATH));
  });

  it('shows the retry-after wait when the server rate limits creation', async () => {
    await openHome();
    mockServer.use(
      http.post(ENDPOINTS_PATH, () =>
        jsonResponse({ error: { code: 'rate_limited', message: 'slow down' } }, 429, { 'Retry-After': '7' }),
      ),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Create endpoint' }));

    expect(await screen.findByText('Too many requests. Try again in 7 s.')).toBeTruthy();
  });

  it('maps a server field error onto the header row it names and moves focus there', async () => {
    await openHome();
    mockServer.use(
      http.post(ENDPOINTS_PATH, () =>
        errorResponse(422, 'validation_failed', 'response.headers.0.name not allowed', [
          { field: 'response.headers.0.name', message: 'not allowed' },
        ]),
      ),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Custom response' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add header' }));
    fireEvent.change(screen.getByLabelText('Header 1 name'), { target: { value: 'X-Reply' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create with this response' }));

    const nameInput = await screen.findByLabelText('Header 1 name');
    await waitFor(() => expect(nameInput.getAttribute('aria-invalid')).toBe('true'));
    await waitFor(() => expect(document.activeElement).toBe(nameInput));
    expect(screen.getByText('This value is not valid.')).toBeTruthy();
  });

  it('blocks submission and marks the status field when the client-side range is broken', async () => {
    const { router } = await openHome();
    fireEvent.click(screen.getByRole('button', { name: 'Custom response' }));
    fireEvent.change(screen.getByLabelText('Status code'), { target: { value: '99' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create with this response' }));

    expect(await screen.findByText('Use a status code from 100 to 599.')).toBeTruthy();
    expect(screen.getByLabelText('Status code').getAttribute('aria-invalid')).toBe('true');
    expect(router.state.location.pathname).toBe('/');
  });

  it('names a remembered endpoint by its stored title, and a local label wins over it', async () => {
    const entry: MyEndpoint = {
      id: ENDPOINT_ID,
      label: null,
      title: 'Stored title',
      added: 1,
      lastOpened: Date.now(),
      lastSeenCaptureId: null,
    };
    window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, serializeEndpoints([entry]));
    const view = await openHome();

    expect(within(listOf()).getByText('Stored title')).toBeTruthy();
    expect(within(listOf()).getByText(SHORT_ID)).toBeTruthy();
    view.unmount();

    window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, serializeEndpoints([{ ...entry, label: 'my label' }]));
    await openHome();
    expect(within(listOf()).getByText('my label')).toBeTruthy();
    expect(within(listOf()).queryByText('Stored title')).toBeNull();
  });

  it('lists remembered endpoints and saves an inline label', async () => {
    seedOne();
    await openHome();
    const main = within(listOf());

    fireEvent.click(main.getByRole('button', { name: `Rename ${SHORT_ID}` }));
    fireEvent.change(screen.getByLabelText(`Label for ${SHORT_ID}`), { target: { value: 'stripe-dev' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    expect(within(listOf()).getByText('stripe-dev')).toBeTruthy();
    expect(parseEndpoints(window.localStorage.getItem(ENDPOINTS_STORAGE_KEY))[0]?.label).toBe('stripe-dev');
    await waitFor(() => expect(document.activeElement?.textContent).toBe('Rename'));
  });

  it('removes an endpoint from my endpoints without touching the server copy', async () => {
    seedOne('stripe-dev');
    await openHome();

    fireEvent.click(within(listOf()).getByRole('button', { name: `Remove stripe-dev from my endpoints` }));

    expect(
      within(listOf()).getByText('No endpoints yet. Endpoints you create appear here, saved in this browser only.'),
    ).toBeTruthy();
    expect(parseEndpoints(window.localStorage.getItem(ENDPOINTS_STORAGE_KEY))).toEqual([]);
  });
});
