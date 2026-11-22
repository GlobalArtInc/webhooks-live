import { act, cleanup, configure, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearMarkedCapture } from '../../features/diff';
import { forgetSelection } from '../../features/endpoint/selectionMemory';
import { captureIdFor, captureSeries } from '../../features/endpoint/testFixtures';
import { renderAppAt } from '../../features/endpoints/renderApp';
import { FakeEventSource } from '../../features/live/fakeEventSource';
import { summaryFor } from '../../features/live/liveFixtures';
import { resetLivePrefsState } from '../../features/live/prefs';
import { resetLiveStores } from '../../features/live/store';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

const ENDPOINT_PATH = `/e/${ENDPOINT_ID}`;
const CLIENT_HEIGHT_PX = 360;
const BASE_TITLE = 'webhooks.live';
const ENDPOINT_TITLE = 'stripe test — webhooks.live';
const ASYNC_TIMEOUT_MS = 5000;
const DEFAULT_ASYNC_TIMEOUT_MS = 1000;

const streams: FakeEventSource[] = [];

class RecordedEventSource extends FakeEventSource {
  constructor(url: string) {
    super(url);
    streams.push(this);
  }
}

function stream(): FakeEventSource {
  const latest = streams.at(-1);
  if (latest === undefined) {
    throw new Error('the endpoint view did not open a stream');
  }
  return latest;
}

function listbox(): HTMLElement {
  return screen.getByRole('listbox', { name: 'Captures' });
}

function optionFor(seq: number): HTMLElement {
  const match = within(listbox())
    .getAllByRole('option')
    .find((option) => option.id.endsWith(captureIdFor(seq)));
  if (match === undefined) {
    throw new Error(`no row for capture ${seq}`);
  }
  return match;
}

function announcerText(): string {
  return (
    screen.getAllByRole('status').find((region) => region.getAttribute('aria-live') === 'polite')?.textContent ?? ''
  );
}

async function openEndpoint(count: number, path: string = ENDPOINT_PATH) {
  seedEndpoint(endpointFixture, captureSeries(count));
  const view = renderAppAt(path);
  await screen.findByRole('listbox', { name: 'Captures' });
  await waitFor(() => expect(streams.length).toBeGreaterThan(0));
  act(() => {
    stream().open();
  });
  return view;
}

function arrive(seq: number): void {
  act(() => {
    stream().send('capture', summaryFor(seq), String(seq));
  });
}

function setHidden(value: boolean): void {
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => value });
  act(() => {
    document.dispatchEvent(new Event('visibilitychange'));
  });
}

describe('endpoint view live updates', () => {
  beforeAll(() => {
    configure({ asyncUtilTimeout: ASYNC_TIMEOUT_MS });
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => CLIENT_HEIGHT_PX });
  });

  afterAll(() => {
    configure({ asyncUtilTimeout: DEFAULT_ASYNC_TIMEOUT_MS });
    Reflect.deleteProperty(HTMLElement.prototype, 'clientHeight');
  });

  beforeEach(() => {
    streams.length = 0;
    document.title = BASE_TITLE;
    window.localStorage.clear();
    vi.stubGlobal('EventSource', RecordedEventSource);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    Reflect.deleteProperty(document, 'hidden');
    window.localStorage.clear();
    resetLiveStores();
    resetLivePrefsState();
    forgetSelection(ENDPOINT_ID);
    clearMarkedCapture(ENDPOINT_ID);
  });

  it('shows a new capture at the top without moving the selection, and announces it', async () => {
    await openEndpoint(3);
    expect(optionFor(3).getAttribute('aria-selected')).toBe('true');

    arrive(4);

    await waitFor(() => expect(optionFor(4)).toBeTruthy());
    expect(optionFor(4).getAttribute('aria-posinset')).toBe('1');
    expect(optionFor(4).getAttribute('aria-selected')).toBe('false');
    expect(optionFor(3).getAttribute('aria-selected')).toBe('true');
    expect(optionFor(4).getAttribute('data-unread')).toBe('true');
    expect(within(optionFor(4)).getByText('Unread')).toBeTruthy();
    expect(screen.getByText('Live')).toBeTruthy();
    expect(announcerText()).toBe('New capture: POST /hook/4');
  });

  it('does not take keyboard focus when a capture arrives', async () => {
    await openEndpoint(3);
    act(() => {
      listbox().focus();
    });

    arrive(4);

    await waitFor(() => expect(optionFor(4)).toBeTruthy());
    expect(document.activeElement).toBe(listbox());
  });

  it('ignores a duplicate of a capture that is already listed', async () => {
    await openEndpoint(3);

    arrive(3);
    arrive(4);
    arrive(4);

    await waitFor(() => expect(optionFor(4)).toBeTruthy());
    expect(within(listbox()).getAllByRole('option')).toHaveLength(4);
    expect(optionFor(4).getAttribute('aria-setsize')).toBe('4');
  });

  it('follows the newest capture once Follow is switched on', async () => {
    await openEndpoint(3);
    fireEvent.click(screen.getByRole('button', { name: 'Follow' }));

    arrive(4);

    await waitFor(() => expect(optionFor(4).getAttribute('aria-selected')).toBe('true'));
    expect(optionFor(4).getAttribute('data-unread')).toBeNull();
  });

  it('follows through the URL when a capture is selected, and pauses on an older selection', async () => {
    const { router } = await openEndpoint(3, `${ENDPOINT_PATH}/c/${captureIdFor(3)}`);
    fireEvent.click(screen.getByRole('button', { name: 'Follow' }));

    arrive(4);
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(4)}`));

    fireEvent.click(optionFor(2));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`));
    expect(screen.getByRole('button', { name: /Follow\s*paused/ })).toBeTruthy();

    arrive(5);
    await waitFor(() => expect(optionFor(5)).toBeTruthy());
    expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(2)}`);
    expect(optionFor(2).getAttribute('aria-selected')).toBe('true');

    fireEvent.click(screen.getByRole('button', { name: /Follow\s*paused/ }));
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(5)}`));
    arrive(6);
    await waitFor(() => expect(router.state.location.pathname).toBe(`${ENDPOINT_PATH}/c/${captureIdFor(6)}`));
  });

  it('counts arrivals in the tab title while hidden and clears it when the tab returns', async () => {
    await openEndpoint(3);
    setHidden(true);

    arrive(4);
    arrive(5);

    await waitFor(() => expect(document.title).toBe(`(2) ${ENDPOINT_TITLE}`));
    expect(announcerText()).toBe('');

    setHidden(false);

    await waitFor(() => expect(document.title).toBe(ENDPOINT_TITLE));
  });

  it('offers a new-captures pill when scrolled away and scrolls back to the top on click', async () => {
    await openEndpoint(60);
    const scroller = listbox().parentElement;
    if (scroller === null) {
      throw new Error('the listbox has no scroll container');
    }
    Object.defineProperty(scroller, 'scrollTop', { configurable: true, writable: true, value: 400 });
    fireEvent.scroll(scroller);

    arrive(61);
    arrive(62);

    const pill = await screen.findByRole('button', { name: '2 new captures, scroll to the top' });
    expect(screen.getByText('2 new')).toBeTruthy();
    fireEvent.click(pill);

    expect(scroller.scrollTop).toBe(0);
    await waitFor(() => expect(screen.queryByRole('button', { name: /scroll to the top/ })).toBeNull());
  });

  it('shows no pill while the list is at the top', async () => {
    await openEndpoint(3);

    arrive(4);

    await waitFor(() => expect(optionFor(4)).toBeTruthy());
    expect(screen.queryByRole('button', { name: /scroll to the top/ })).toBeNull();
  });

  it('stays silent when announcements are muted', async () => {
    await openEndpoint(3);
    fireEvent.click(screen.getByRole('button', { name: 'Announce arrivals' }));

    arrive(4);

    await waitFor(() => expect(optionFor(4)).toBeTruthy());
    expect(announcerText()).toBe('');
  });

  it('removes a deleted capture and shows the gone state when the endpoint is deleted', async () => {
    await openEndpoint(3);

    act(() => {
      stream().send('capture_deleted', { capture_id: captureIdFor(2), seq: 2 });
    });
    await waitFor(() => expect(within(listbox()).getAllByRole('option')).toHaveLength(2));

    act(() => {
      stream().send('endpoint_deleted', {});
    });

    expect(await screen.findByText('This endpoint is gone')).toBeTruthy();
  });

  it('shows the reconnecting state while the stream is down', async () => {
    await openEndpoint(3);

    act(() => {
      stream().fail(false);
    });

    expect(await screen.findByText('Reconnecting...')).toBeTruthy();
  });
});
