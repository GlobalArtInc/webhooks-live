import { act, cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMarkedCapture } from '../../features/diff';
import { WIDE_QUERY } from '../../features/endpoint/constants';
import { sampleCurl } from '../../features/endpoint/sampleCurl';
import { forgetSelection } from '../../features/endpoint/selectionMemory';
import { captureIdFor, captureSeries } from '../../features/endpoint/testFixtures';
import { renderAppAt } from '../../features/endpoints/renderApp';
import { UNDO_WINDOW_MS } from '../../features/list';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { emptyResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const UNKNOWN_CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4aff';
const CAPTURE_DELETE = '/api/v1/endpoints/:endpointId/captures/:captureId';
const CLIENT_HEIGHT_PX = 360;

function seed(count: number): void {
  seedEndpoint(endpointFixture, captureSeries(count));
}

function nth<T>(items: T[], index: number): T {
  const item = items[index];
  if (item === undefined) {
    throw new Error(`no element at index ${index}`);
  }
  return item;
}

function listboxOf(): HTMLElement {
  return screen.getByRole('listbox', { name: 'Captures' });
}

function optionsOf(): HTMLElement[] {
  return within(listboxOf()).getAllByRole('option');
}

function optionAt(position: number): HTMLElement {
  return nth(
    optionsOf().filter((option) => option.getAttribute('aria-posinset') === String(position)),
    0,
  );
}

function rowButtons(name: string): HTMLElement[] {
  const scroller = listboxOf().parentElement;
  if (scroller === null) {
    throw new Error('the listbox has no scroll container');
  }
  return within(scroller).getAllByRole('button', { name });
}

function recordDeletes(): string[] {
  const deleted: string[] = [];
  mockServer.use(
    http.delete(CAPTURE_DELETE, ({ params }) => {
      deleted.push(String(params.captureId));
      return emptyResponse(204);
    }),
  );
  return deleted;
}

async function openEndpoint(path: string = ENDPOINT_PATH) {
  const view = renderAppAt(path);
  await screen.findByRole('listbox', { name: 'Captures' });
  return view;
}

describe('endpoint view', () => {
  beforeAll(() => {
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get: () => CLIENT_HEIGHT_PX,
    });
  });

  afterAll(() => {
    Reflect.deleteProperty(HTMLElement.prototype, 'clientHeight');
  });

  beforeEach(() => {
    vi.stubGlobal('matchMedia', (query: string): MediaQueryList => ({
      matches: query === WIDE_QUERY,
      media: query,
      onchange: null,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      dispatchEvent: () => false,
    }));
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    forgetSelection(ENDPOINT_ID);
    clearMarkedCapture(ENDPOINT_ID);
  });

  it('lists the newest capture first with the listbox and option attributes', async () => {
    seed(3);
    await openEndpoint();

    const options = optionsOf();
    expect(options.map((option) => option.getAttribute('aria-posinset'))).toEqual(['1', '2', '3']);
    expect(options.every((option) => option.getAttribute('aria-setsize') === '3')).toBe(true);
    expect(nth(options, 0).getAttribute('aria-selected')).toBe('true');
    expect(listboxOf().getAttribute('aria-activedescendant')).toBe(nth(options, 0).id);
  });

  it('selects the capture named in the URL and leaves the URL as it is', async () => {
    seed(3);
    const { router } = await openEndpoint(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`);

    await waitFor(() => expect(optionAt(2).getAttribute('aria-selected')).toBe('true'));
    expect(optionAt(1).getAttribute('aria-selected')).toBe('false');
    expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`);
  });

  it('selects the newest capture without writing it into the URL', async () => {
    seed(3);
    const { router } = await openEndpoint();

    await waitFor(() => expect(optionAt(1).getAttribute('aria-selected')).toBe('true'));
    expect(router.state.location.pathname).toBe(ENDPOINT_PATH);
  });

  it('shows the not-found state inside the detail pane for an unknown capture id', async () => {
    seed(3);
    await openEndpoint(`${ENDPOINT_PATH}/c/${UNKNOWN_CAPTURE_ID}`);

    expect(await screen.findByText('This capture was not found')).toBeTruthy();
    expect(listboxOf()).toBeTruthy();
  });

  it('loads later pages until a deep-linked capture is listed and selects it', async () => {
    seed(250);
    await openEndpoint(`${ENDPOINT_PATH}/c/${captureIdFor(10)}`);

    await waitFor(() => expect(optionAt(241).getAttribute('aria-selected')).toBe('true'));
    expect(optionAt(241).getAttribute('aria-setsize')).toBe('250');
  });

  it('walks the loaded captures with first, previous, next and last', async () => {
    seed(3);
    const { router } = await openEndpoint();
    const pager = screen.getByRole('navigation', { name: 'Capture position' });

    fireEvent.click(within(pager).getByRole('button', { name: 'Last' }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(1)}`));
    fireEvent.click(within(pager).getByRole('button', { name: 'Previous' }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`));
    fireEvent.click(within(pager).getByRole('button', { name: 'First' }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(3)}`));
    fireEvent.click(within(pager).getByRole('button', { name: 'Next' }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`));
    expect(within(pager).getByText('2 / 3')).toBeTruthy();
  });

  it('Next past the loaded end loads the next page and selects the capture after the last loaded one', async () => {
    seed(150);
    const { router } = await openEndpoint(`${ENDPOINT_PATH}/c/${captureIdFor(51)}`);
    const pager = screen.getByRole('navigation', { name: 'Capture position' });

    fireEvent.click(within(pager).getByRole('button', { name: 'Next' }));

    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(50)}`));
  });

  it('Undo cancels the pending delete: the capture returns and no DELETE is sent', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'], shouldAdvanceTime: true });
    seed(3);
    const deleted = recordDeletes();
    await openEndpoint();

    fireEvent.click(nth(rowButtons('Delete capture'), 0));
    await waitFor(() => expect(optionsOf()).toHaveLength(2));
    expect(screen.getByText('Capture deleted')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await waitFor(() => expect(optionsOf()).toHaveLength(3));

    act(() => {
      vi.advanceTimersByTime(UNDO_WINDOW_MS + 1);
    });
    expect(deleted).toEqual([]);
  });

  it('an expired Undo window sends the DELETE for the removed capture', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'], shouldAdvanceTime: true });
    seed(3);
    const deleted = recordDeletes();
    await openEndpoint();

    fireEvent.click(nth(rowButtons('Delete capture'), 0));
    await waitFor(() => expect(optionsOf()).toHaveLength(2));
    act(() => {
      vi.advanceTimersByTime(UNDO_WINDOW_MS);
    });

    await waitFor(() => expect(deleted).toEqual([captureIdFor(3)]));
    expect(optionsOf()).toHaveLength(2);
  });

  it('deleting the selected capture selects its neighbour, and Undo selects the capture again', async () => {
    seed(3);
    const { router } = await openEndpoint(`${ENDPOINT_PATH}/c/${captureIdFor(3)}`);
    recordDeletes();

    fireEvent.click(nth(rowButtons('Delete capture'), 0));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`));

    fireEvent.click(screen.getByRole('button', { name: 'Undo' }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(3)}`));
    expect(optionsOf()).toHaveLength(3);
  });

  it('settings replaces the list with the workspace, and going back keeps the selected capture', async () => {
    seed(3);
    const { router } = await openEndpoint(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`);

    fireEvent.click(screen.getByRole('link', { name: 'Settings' }));
    expect(await screen.findByRole('heading', { name: 'General' })).toBeTruthy();
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/settings/general`));
    expect(screen.queryByRole('listbox', { name: 'Captures' })).toBeNull();

    fireEvent.click(screen.getByRole('link', { name: 'Back to captures' }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`));
    expect(optionAt(2).getAttribute('aria-selected')).toBe('true');
  });

  it('Delete all asks first, then clears the endpoint into the empty state', async () => {
    seed(3);
    await openEndpoint();

    fireEvent.click(screen.getByRole('button', { name: 'Delete all' }));
    const dialog = await screen.findByRole('dialog', { name: 'Delete all captures?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(optionsOf()).toHaveLength(3);

    fireEvent.click(screen.getByRole('button', { name: 'Delete all' }));
    const confirm = await screen.findByRole('dialog', { name: 'Delete all captures?' });
    fireEvent.click(within(confirm).getByRole('button', { name: 'Delete all' }));

    expect(await screen.findByText('Waiting for the first request')).toBeTruthy();
  });

  it('an empty endpoint shows its capture URL and a curl example that sends a test request to it', async () => {
    seedEndpoint(endpointFixture, []);
    renderAppAt(ENDPOINT_PATH);

    expect(await screen.findByText('Waiting for the first request')).toBeTruthy();
    expect(screen.getByText(/^curl /).textContent).toBe(sampleCurl(endpointFixture.url));
  });

  it('marks one capture for compare and opens the diff route when a second one is chosen', async () => {
    seed(3);
    const { router } = await openEndpoint();

    fireEvent.click(nth(rowButtons('Mark for compare'), 2));
    await waitFor(() => expect(rowButtons('Compare with marked')).toHaveLength(2));
    fireEvent.click(nth(rowButtons('Compare with marked'), 0));

    await waitFor(() =>
      expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/diff/${captureIdFor(1)}/${captureIdFor(3)}`),
    );
  });

  it('applies the filters named in the link and says so when nothing matches', async () => {
    seed(3);
    const { router } = await openEndpoint(`${ENDPOINT_PATH}?method=GET`);

    expect(await screen.findByText('No captures match these filters.')).not.toBeNull();
    expect(router.state.location.href).toContain('method=GET');
  });

  it('keeps the filters in the URL when a filtered capture is chosen', async () => {
    seed(3);
    const { router } = await openEndpoint(`${ENDPOINT_PATH}?method=POST`);

    await waitFor(() => expect(optionAt(1).getAttribute('aria-selected')).toBe('true'));
    fireEvent.click(optionAt(2));

    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`));
    expect(router.state.location.href).toContain('method=POST');
  });
});
