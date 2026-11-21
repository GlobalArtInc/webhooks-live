import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { endpointFixture } from '../../test/fixtures/endpoint';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { I18nProvider } from '../../i18n/provider';
import { CreateTokenDialog } from './CreateTokenDialog';
import { tokenQueryKeys } from '../../api/tokens';
import { fakeSecret } from './testFixtures';
import { limitFailure, newTokenStore, rateFailure, tokenHandlers, type TokenStore } from './testHandlers';

registerMockServer();

const OTHER_ENDPOINT_ID = 'abcdefghjkmnpqrstvwxyz01';
const CONSOLE_METHODS = ['log', 'info', 'warn', 'error', 'debug'] as const;

function Host() {
  const [open, setOpen] = useState(true);
  return (
    <>
      <p>{open ? 'dialog open' : 'dialog closed'}</p>
      {open && <CreateTokenDialog onClose={() => setOpen(false)} />}
    </>
  );
}

function renderDialog(store: TokenStore) {
  mockServer.use(...tokenHandlers(store));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider bundles={{}}>
        <Host />
      </I18nProvider>
    </QueryClientProvider>,
  );
  return queryClient;
}

function placesHolding(secret: string, queryClient: QueryClient): string[] {
  const places: Record<string, string> = {
    queryCache: JSON.stringify(
      queryClient
        .getQueryCache()
        .getAll()
        .map((query) => query.state),
    ),
    mutationCache: JSON.stringify(
      queryClient
        .getMutationCache()
        .getAll()
        .map((mutation) => mutation.state),
    ),
    localStorage: JSON.stringify({ ...window.localStorage }),
    sessionStorage: JSON.stringify({ ...window.sessionStorage }),
    url: window.location.href,
    historyState: JSON.stringify(window.history.state),
    markup: document.body.innerHTML,
    fields: JSON.stringify(Array.from(document.querySelectorAll('input')).map((input) => input.value)),
  };
  return Object.entries(places)
    .filter(([, text]) => text.includes(secret))
    .map(([name]) => name);
}

async function createToken(name: string): Promise<HTMLInputElement> {
  fireEvent.change(await screen.findByLabelText('Name'), { target: { value: name } });
  fireEvent.click(screen.getByRole('button', { name: 'Create' }));
  const field = await screen.findByLabelText('API token secret');
  if (!(field instanceof HTMLInputElement)) {
    throw new Error('the secret field is not an input');
  }
  return field;
}

const DOM_PLACES: readonly string[] = ['markup', 'fields'];

const writeText = vi.fn<(text: string) => Promise<void>>();

beforeEach(() => {
  writeText.mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
  window.localStorage.clear();
  window.sessionStorage.clear();
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  writeText.mockReset();
});

describe('create token dialog', () => {
  it('detects a secret in every place the leak check inspects', () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(['probe'], { value: fakeSecret() });
    window.localStorage.setItem('probe', fakeSecret());
    window.sessionStorage.setItem('probe', fakeSecret());
    window.history.replaceState({ probe: fakeSecret() }, '', `/?probe=${fakeSecret()}`);

    expect(placesHolding(fakeSecret(), queryClient)).toEqual([
      'queryCache',
      'localStorage',
      'sessionStorage',
      'url',
      'historyState',
    ]);
    window.history.replaceState(null, '', '/');
  });

  it('posts the chosen scopes and expiry and shows the secret masked until Show is pressed', async () => {
    const store = newTokenStore();
    renderDialog(store);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Rules write' }));
    fireEvent.change(screen.getByLabelText('Expires'), { target: { value: '30' } });

    const field = await createToken('  ci-runner  ');

    expect(store.created).toEqual([{ name: 'ci-runner', scopes: ['read', 'rules:write'], expires_in_days: 30 }]);
    expect(field.value).not.toContain('FAKE');
    expect(document.activeElement).toBe(field);
    expect(screen.getByText(/shown only once/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Show' }));
    expect(field.value).toBe(fakeSecret());
    fireEvent.click(screen.getByRole('button', { name: 'Hide' }));
    expect(field.value).not.toContain('FAKE');
  });

  it('keeps the secret out of the query cache, storage, URL, history and logs, during and after display', async () => {
    const consoleSpies = CONSOLE_METHODS.map((method) => vi.spyOn(console, method).mockImplementation(() => undefined));
    const queryClient = renderDialog(newTokenStore());
    const secret = fakeSecret();

    await createToken('ci-runner');
    await waitFor(() => expect(queryClient.getMutationCache().getAll()).toHaveLength(0));
    expect(placesHolding(secret, queryClient)).toEqual([]);
    fireEvent.click(screen.getByRole('button', { name: 'Show' }));
    expect(placesHolding(secret, queryClient).filter((place) => !DOM_PLACES.includes(place))).toEqual([]);

    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Copied' })).toBeTruthy());
    expect(writeText).toHaveBeenCalledWith(secret);
    fireEvent.click(screen.getByRole('button', { name: 'Done' }));
    await screen.findByText('dialog closed');

    expect(placesHolding(secret, queryClient)).toEqual([]);
    expect(queryClient.getMutationCache().getAll()).toHaveLength(0);
    expect(screen.queryByLabelText('API token secret')).toBeNull();
    for (const spy of consoleSpies) {
      expect(JSON.stringify(spy.mock.calls)).not.toContain(secret);
    }
  });

  it('asks before closing a secret that was not copied, and Go back keeps it', async () => {
    renderDialog(newTokenStore());
    await createToken('ci-runner');

    fireEvent.click(screen.getByRole('button', { name: 'Done' }));

    expect(await screen.findByText('Close without copying?')).toBeTruthy();
    expect(screen.getByText('The token cannot be shown again.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Go back' }));
    expect(screen.queryByText('Close without copying?')).toBeNull();
    expect(screen.getByLabelText('API token secret')).toBeTruthy();

    fireEvent.keyDown(screen.getByLabelText('API token secret'), { key: 'Escape' });
    fireEvent.click(await screen.findByRole('button', { name: 'Close anyway' }));
    expect(await screen.findByText('dialog closed')).toBeTruthy();
  });

  it('shows validation messages and sends nothing for an empty name and no scopes', async () => {
    const store = newTokenStore();
    renderDialog(store);
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Read' }));

    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(await screen.findByText('Enter a name.')).toBeTruthy();
    expect(screen.getByText('Choose at least one scope.')).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByLabelText('Name'));
    expect(store.created).toEqual([]);
  });

  it('sends only the endpoints chosen in the picker and requires at least one', async () => {
    mockServer.use(
      http.get('/api/v1/endpoints', () =>
        jsonResponse({
          items: [endpointFixture, { ...endpointFixture, id: OTHER_ENDPOINT_ID, title: null }],
          next_cursor: null,
        }),
      ),
    );
    const store = newTokenStore();
    renderDialog(store);
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'ci' } });
    fireEvent.click(screen.getByRole('radio', { name: 'Only the endpoints I choose' }));

    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(await screen.findByText('Choose at least one endpoint.')).toBeTruthy();
    expect(store.created).toEqual([]);

    const list = await screen.findByRole('list', { name: 'Allowed endpoints' });
    fireEvent.click(within(list).getByRole('checkbox', { name: /Untitled endpoint/ }));
    expect(screen.getByText('1 of 50 selected')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    await screen.findByLabelText('API token secret');
    expect(store.created[0]?.endpoint_ids).toEqual([OTHER_ENDPOINT_ID]);
  });

  it('explains the 20 active token limit and keeps the form open', async () => {
    const store = newTokenStore();
    store.createFailure = limitFailure();
    renderDialog(store);
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'ci' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(
      await screen.findByText('You already have 20 active tokens. Revoke one before creating another.'),
    ).toBeTruthy();
    expect(screen.getByLabelText('Name')).toBeTruthy();
  });

  it('explains the hourly creation quota with the reset time', async () => {
    const store = newTokenStore();
    store.createFailure = rateFailure(new Date(Date.now() + 15 * 60_000).toISOString());
    renderDialog(store);
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'ci' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    const alert = await screen.findByText(/You can create 10 tokens per hour\. Try again/);
    expect(alert.textContent).toMatch(/in 15 minutes/);
  });

  it('sends one request when the form is submitted twice before a re-render', async () => {
    const store = newTokenStore();
    renderDialog(store);
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'ci' } });
    const form = screen.getByLabelText('Name').closest('form');
    if (form === null) {
      throw new Error('the form is missing');
    }

    act(() => {
      form.requestSubmit();
      form.requestSubmit();
    });

    await screen.findByLabelText('API token secret');
    expect(store.created).toHaveLength(1);
  });

  it('drops chosen endpoints that are no longer in the loaded list before sending', async () => {
    mockServer.use(http.get('/api/v1/endpoints', () => jsonResponse({ items: [endpointFixture], next_cursor: null })));
    const store = newTokenStore();
    const queryClient = renderDialog(store);
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'ci' } });
    fireEvent.click(screen.getByRole('radio', { name: 'Only the endpoints I choose' }));
    const list = await screen.findByRole('list', { name: 'Allowed endpoints' });
    fireEvent.click(within(list).getByRole('checkbox'));
    expect(screen.getByText('1 of 50 selected')).toBeTruthy();

    act(() => {
      queryClient.setQueryData(tokenQueryKeys.ownedEndpoints(), {
        pages: [{ items: [], next_cursor: null }],
        pageParams: [null],
      });
    });
    await screen.findByText('You do not own any endpoints yet.');
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));

    expect(await screen.findByText('Choose at least one endpoint.')).toBeTruthy();
    expect(store.created).toEqual([]);
  });

  it('keeps the chosen endpoints visible when a later refresh of the endpoint list fails', async () => {
    mockServer.use(http.get('/api/v1/endpoints', () => jsonResponse({ items: [endpointFixture], next_cursor: null })));
    const queryClient = renderDialog(newTokenStore());
    fireEvent.click(await screen.findByRole('radio', { name: 'Only the endpoints I choose' }));
    const list = await screen.findByRole('list', { name: 'Allowed endpoints' });
    fireEvent.click(within(list).getByRole('checkbox'));
    mockServer.use(http.get('/api/v1/endpoints', () => jsonResponse({ error: { code: 'x', message: 'x' } }, 500)));

    await queryClient.refetchQueries({ queryKey: tokenQueryKeys.ownedEndpoints() }).catch(() => undefined);

    expect(screen.getByRole('list', { name: 'Allowed endpoints' })).toBeTruthy();
    expect(screen.queryByText('Your endpoints could not be loaded.')).toBeNull();
  });

  it('lets Cancel abort a pending request and then points at the token list', async () => {
    const store = newTokenStore();
    store.hang = true;
    renderDialog(store);
    fireEvent.change(await screen.findByLabelText('Name'), { target: { value: 'ci' } });

    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    await screen.findByRole('button', { name: 'Creating...' });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    expect(await screen.findByText(/the token may have been created/)).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Check the token list' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Create' })).toBeTruthy();
  });

  it('hints that Endpoints write is of no use together with an endpoint restriction', async () => {
    mockServer.use(http.get('/api/v1/endpoints', () => jsonResponse({ items: [], next_cursor: null })));
    renderDialog(newTokenStore());
    fireEvent.click(await screen.findByRole('checkbox', { name: 'Endpoints write' }));
    expect(screen.queryByText(/cannot create new endpoints/)).toBeNull();

    fireEvent.click(screen.getByRole('radio', { name: 'Only the endpoints I choose' }));

    expect(await screen.findByText(/cannot create new endpoints/)).toBeTruthy();
  });
});
