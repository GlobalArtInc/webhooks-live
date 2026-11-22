import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, RouterProvider } from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createAppRouter } from '../../app/router';
import { I18nProvider } from '../../i18n/provider';
import { openApiFixture } from '../../test/fixtures/openapi';
import { openApiStore } from '../../test/msw/openapi';
import { registerMockServer } from '../../test/msw/server';
import { parseOpenApiDocument, openApiQueryKey } from './document';
import { buildReference } from './reference';
import { operationAnchorId } from './anchors';
import { renderAppAt } from '../endpoints/renderApp';
import { clearComposerSeed, peekComposerSeed } from '../composer/seedHandoff';

registerMockServer();

vi.mock('../../routes/compose/ComposeScreen', () => ({
  ComposeScreen: () => <h1>Composer stub</h1>,
}));

const document = parseOpenApiDocument(openApiFixture);
const reference = buildReference(document);
const OPERATION_COUNT = reference.operations.length;
const scrollIntoView = vi.fn();
const NOT_FOUND_TITLE = 'Page not found';

beforeEach(() => {
  scrollIntoView.mockReset();
  Element.prototype.scrollIntoView = scrollIntoView;
});

afterEach(() => {
  cleanup();
  clearComposerSeed();
});

describe('API reference page', () => {
  it('renders every operation of the served document with its navigation entry', async () => {
    renderAppAt('/docs/api');

    const articles = await screen.findAllByRole('article');
    expect(articles).toHaveLength(OPERATION_COUNT);
    const nav = screen.getByRole('navigation', { name: 'API operations' });
    expect(within(nav).getAllByRole('link')).toHaveLength(OPERATION_COUNT);
    expect(screen.getByRole('heading', { level: 1, name: 'webhooks.live API' })).toBeTruthy();
    expect(screen.getByText('Version 0.1.0-dev')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Download OpenAPI' }).getAttribute('href')).toBe('/api/v1/openapi.json');
    expect(openApiStore.requests).toBe(1);
  });

  it('scrolls to the operation of a deep link and focuses its heading', async () => {
    renderAppAt('/docs/api/rules_create');

    const heading = await screen.findByRole('heading', { level: 3, name: /POST\s*\/endpoints\/\{id\}\/rules$/ });
    await waitFor(() => {
      expect(globalThis.document.activeElement).toBe(heading);
    });
    expect(scrollIntoView).toHaveBeenCalledTimes(1);
    expect((scrollIntoView.mock.contexts[0] as HTMLElement).id).toBe(operationAnchorId('rules_create'));
  });

  it('says so when the deep link names no operation, and still shows the reference', async () => {
    renderAppAt('/docs/api/no_such_operation');

    expect(await screen.findByText(/no operation named no_such_operation/)).toBeTruthy();
    expect(screen.getAllByRole('article')).toHaveLength(OPERATION_COUNT);
    expect(scrollIntoView).not.toHaveBeenCalled();
  });

  it('narrows the list with the filter and keeps it in the q parameter', async () => {
    const { router } = renderAppAt('/docs/api');
    await screen.findAllByRole('article');

    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter operations' }), { target: { value: 'aliases_' } });

    const expected = reference.operations.filter((operation) => operation.id.includes('aliases_')).length;
    await waitFor(() => {
      expect(screen.getAllByRole('article')).toHaveLength(expected);
    });
    await waitFor(() => {
      expect(router.state.location.search).toEqual({ q: 'aliases_' });
    });

    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter operations' }), { target: { value: 'zzzz' } });
    await waitFor(() => {
      expect(screen.queryAllByRole('article')).toHaveLength(0);
    });
    expect(screen.getAllByText('No operation matches the filter.').length).toBeGreaterThan(0);
  });

  it('lets a schema be expanded and announces a copied sample', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    renderAppAt('/docs/api/tokens_create');
    const article = await screen.findByRole('article', { name: /POST\s*\/tokens/ });

    fireEvent.click(within(article).getByRole('button', { name: /^Expand POST \/tokens 201/ }));
    expect(within(article).getByRole('button', { name: /^Collapse POST \/tokens 201/ })).toBeTruthy();

    fireEvent.click(within(article).getByRole('button', { name: /^Copy the curl sample of POST \/tokens$/ }));
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledTimes(1);
    });
    expect(String(writeText.mock.calls[0]?.[0])).toContain("--url 'http://localhost:3000/api/v1/tokens'");
    expect(
      (await within(article).findAllByRole('status')).some(
        (status) => status.textContent === 'Copied to the clipboard.',
      ),
    ).toBe(true);
  });

  it('hands method, URL, headers and body to the composer seed and opens the compose route', async () => {
    const { router } = renderAppAt('/docs/api/tokens_create');
    const article = await screen.findByRole('article', { name: /POST\s*\/tokens/ });

    fireEvent.click(within(article).getByRole('button', { name: 'Open in composer: POST /tokens' }));

    expect(await screen.findByRole('heading', { level: 1, name: 'Composer stub' })).toBeTruthy();
    expect(router.state.location.pathname).toBe('/send');
    const seed = peekComposerSeed();
    expect(seed?.draft.method).toBe('POST');
    expect(seed?.draft.url).toBe('http://localhost:3000/api/v1/tokens');
    expect(seed?.draft.headers.map((row) => [row.name, row.value])).toEqual([['Content-Type', 'application/json']]);
    expect(JSON.parse(seed?.draft.body.data ?? '')).toEqual({ name: 'laptop cli', scopes: ['read'] });
    expect(seed?.banner).toBeNull();
  });

  it('mounts only the header of each operation until it is expanded', async () => {
    const { container } = renderAppAt('/docs/api');
    await screen.findAllByRole('article');

    expect(container.querySelectorAll('pre')).toHaveLength(0);
    expect(container.querySelectorAll('table')).toHaveLength(0);
    const toggle = screen.getByRole('button', { name: 'Show details: GET /usage' });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');

    fireEvent.click(toggle);

    const hide = screen.getByRole('button', { name: 'Hide details: GET /usage' });
    expect(hide.getAttribute('aria-expanded')).toBe('true');
    expect(container.querySelectorAll('pre')).toHaveLength(1);
    expect(globalThis.document.getElementById(hide.getAttribute('aria-controls') ?? '')).toBeTruthy();

    fireEvent.click(hide);
    expect(container.querySelectorAll('pre')).toHaveLength(0);
  });

  it('opens the deep-linked operation and the few results of a narrow filter, nothing else', async () => {
    const { container } = renderAppAt('/docs/api/usage_get');
    await screen.findAllByRole('article');

    expect(container.querySelectorAll('pre')).toHaveLength(1);
    expect(screen.getByRole('button', { name: 'Hide details: GET /usage' })).toBeTruthy();

    fireEvent.change(screen.getByRole('searchbox', { name: 'Filter operations' }), { target: { value: 'aliases_' } });
    await waitFor(() => {
      expect(screen.getAllByRole('article')).toHaveLength(4);
    });
    expect(container.querySelectorAll('pre')).toHaveLength(4);
  });

  it('shows the not-found page when the instance does not serve the document', async () => {
    openApiStore.failure = { status: 404, body: { error: { code: 'not_found', message: 'No such route' } } };
    renderAppAt('/docs/api');

    expect(await screen.findByRole('heading', { level: 1, name: NOT_FOUND_TITLE })).toBeTruthy();
    expect(screen.queryAllByRole('article')).toHaveLength(0);
  });

  it('renders the whole document from the cache within the time budget', async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(openApiQueryKey, document);
    const router = createAppRouter({
      history: createMemoryHistory({ initialEntries: ['/docs/api'] }),
      queryClient,
    });
    await router.load();
    const started = performance.now();
    render(
      <QueryClientProvider client={queryClient}>
        <I18nProvider bundles={{}}>
          <RouterProvider router={router} />
        </I18nProvider>
      </QueryClientProvider>,
    );

    expect(await screen.findAllByRole('article')).toHaveLength(OPERATION_COUNT);
    expect(performance.now() - started).toBeLessThan(2000);
  });
});
