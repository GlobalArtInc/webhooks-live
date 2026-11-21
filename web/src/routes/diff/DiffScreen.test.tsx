import type { QueryClient } from '@tanstack/react-query';
import { createMemoryHistory } from '@tanstack/react-router';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AppProviders, createAppQueryClient } from '../../app/providers';
import { createAppRouter } from '../../app/router';
import { editorText } from '../../features/diff/testDom';
import {
  LARGE_CAPTURE_ID,
  largeCaptureFixture,
  LEFT_CAPTURE_ID,
  leftCaptureFixture,
  RIGHT_CAPTURE_ID,
  rightCaptureFixture,
} from '../../features/diff/testFixtures';
import { ENDPOINT_ID, endpointFixture } from '../../test/fixtures/endpoint';
import { registerMockServer, mockServer } from '../../test/msw/server';
import { errorResponse } from '../../test/msw/responses';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

const NARROW_QUERY = '(max-width: 767px)';

function stubViewport(narrow: boolean): void {
  vi.stubGlobal('matchMedia', (query: string): MediaQueryList => ({
    matches: narrow && query === NARROW_QUERY,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false,
  }));
}

function diffPath(a: string, b: string): string {
  return `/e/${ENDPOINT_ID}/diff/${a}/${b}`;
}

function renderRoute(path: string, queryClient: QueryClient) {
  const router = createAppRouter({
    history: createMemoryHistory({ initialEntries: [path] }),
    queryClient,
  });
  render(<AppProviders queryClient={queryClient} router={router} />);
  return router;
}

function seedCaptures(): void {
  seedEndpoint(endpointFixture, [leftCaptureFixture, rightCaptureFixture, largeCaptureFixture]);
}

describe('DiffScreen', () => {
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('renders the summary for the two captures named in the route', async () => {
    seedCaptures();
    const queryClient = createAppQueryClient();
    renderRoute(diffPath(LEFT_CAPTURE_ID, RIGHT_CAPTURE_ID), queryClient);

    expect(
      await screen.findByText('Headers: 1 changed, 2 added, 0 removed, 2 same, 2 hidden as volatile'),
    ).toBeTruthy();
    await waitFor(() => expect(editorText(document.body)).toContain('X-Retry: 1'));
  });

  it('shows the unified layout by default on narrow screens', async () => {
    stubViewport(true);
    seedCaptures();
    renderRoute(diffPath(LEFT_CAPTURE_ID, RIGHT_CAPTURE_ID), createAppQueryClient());

    expect((await screen.findByRole('button', { name: 'Unified' })).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'Side by side' }).getAttribute('aria-pressed')).toBe('false');
  });

  it('shows the side-by-side layout by default on wide screens and switches on request', async () => {
    stubViewport(false);
    seedCaptures();
    renderRoute(diffPath(LEFT_CAPTURE_ID, RIGHT_CAPTURE_ID), createAppQueryClient());

    expect((await screen.findByRole('button', { name: 'Side by side' })).getAttribute('aria-pressed')).toBe('true');
    fireEvent.click(screen.getByRole('button', { name: 'Unified' }));
    expect(screen.getByRole('button', { name: 'Unified' }).getAttribute('aria-pressed')).toBe('true');
  });

  it('hides and shows volatile headers from the toolbar', async () => {
    seedCaptures();
    renderRoute(diffPath(LEFT_CAPTURE_ID, RIGHT_CAPTURE_ID), createAppQueryClient());

    const toggle = await screen.findByRole('button', { name: 'Hide volatile headers' });
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    await waitFor(() => expect(editorText(document.body)).toContain('X-Retry: 1'));
    expect(editorText(document.body)).not.toContain('Date: Sat, 21 Nov 2026 13:59:40 GMT');

    fireEvent.click(toggle);

    await waitFor(() => expect(editorText(document.body)).toContain('Date: Sat, 21 Nov 2026 13:59:40 GMT'));
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
  });

  it('swaps A and B by navigating to the swapped route', async () => {
    seedCaptures();
    const router = renderRoute(diffPath(LEFT_CAPTURE_ID, RIGHT_CAPTURE_ID), createAppQueryClient());

    fireEvent.click(await screen.findByRole('button', { name: 'Swap A and B' }));

    await waitFor(() => expect(router.state.location.pathname).toBe(diffPath(RIGHT_CAPTURE_ID, LEFT_CAPTURE_ID)));
  });

  it('does not diff a large body until Compare anyway is pressed', async () => {
    seedCaptures();
    renderRoute(diffPath(LEFT_CAPTURE_ID, LARGE_CAPTURE_ID), createAppQueryClient());

    fireEvent.click(await screen.findByRole('button', { name: 'Compare anyway' }));

    await waitFor(() => expect(editorText(document.body)).toContain('"amount"'));
  });

  it('shows the gone state when a capture no longer exists', async () => {
    seedCaptures();
    mockServer.use(
      http.get(`/api/v1/endpoints/${ENDPOINT_ID}/captures/${LEFT_CAPTURE_ID}`, () =>
        errorResponse(410, 'capture_gone', 'Capture is gone.'),
      ),
    );
    renderRoute(diffPath(LEFT_CAPTURE_ID, RIGHT_CAPTURE_ID), createAppQueryClient());

    expect(await screen.findByRole('heading', { level: 1, name: 'This endpoint is gone' })).toBeTruthy();
  });

  it('shows the error state when a capture cannot be loaded', async () => {
    seedCaptures();
    mockServer.use(
      http.get(`/api/v1/endpoints/${ENDPOINT_ID}/captures/${RIGHT_CAPTURE_ID}`, () =>
        errorResponse(400, 'invalid_request', 'Request is invalid.'),
      ),
    );
    renderRoute(diffPath(LEFT_CAPTURE_ID, RIGHT_CAPTURE_ID), createAppQueryClient());

    expect(await screen.findByRole('heading', { level: 1, name: 'This page could not be loaded' })).toBeTruthy();
  });
});
