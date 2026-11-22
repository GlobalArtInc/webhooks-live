import { readFileSync } from 'node:fs';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const ORIGIN = 'https://hooks.example.test';
const ENDPOINT_ID = '7k2m9q4xw8c1d5f3h6j0n2p4';
const CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4e01';
const NOTIFICATION_TARGET = `${ORIGIN}/e/${ENDPOINT_ID}/c/${CAPTURE_ID}`;
const source = readFileSync('public/sw.js', 'utf-8');

interface FakeEvent {
  data?: { json: () => unknown } | null;
  notification?: { close: () => void; data: unknown };
  waitUntil: (work: Promise<unknown>) => void;
}

type Listener = (event: FakeEvent) => void;

interface FakeWindow {
  url: string;
  focus: ReturnType<typeof vi.fn>;
  navigate: ReturnType<typeof vi.fn>;
}

interface Rig {
  listeners: Map<string, Listener>;
  showNotification: ReturnType<typeof vi.fn>;
  matchAll: ReturnType<typeof vi.fn>;
  openWindow: ReturnType<typeof vi.fn>;
  claim: ReturnType<typeof vi.fn>;
  skipWaiting: ReturnType<typeof vi.fn>;
}

interface Fired {
  closed: ReturnType<typeof vi.fn>;
  settled: Promise<unknown>;
}

function boot(windows: FakeWindow[] = []): Rig {
  const listeners = new Map<string, Listener>();
  const rig: Rig = {
    listeners,
    showNotification: vi.fn().mockResolvedValue(undefined),
    matchAll: vi.fn().mockResolvedValue(windows),
    openWindow: vi.fn().mockResolvedValue(null),
    claim: vi.fn().mockResolvedValue(undefined),
    skipWaiting: vi.fn(),
  };
  const scope = {
    addEventListener: (name: string, listener: Listener) => listeners.set(name, listener),
    registration: { showNotification: rig.showNotification },
    clients: { matchAll: rig.matchAll, openWindow: rig.openWindow, claim: rig.claim },
    skipWaiting: rig.skipWaiting,
    location: { origin: ORIGIN },
  };
  new Function('self', source)(scope);
  return rig;
}

function fire(rig: Rig, name: string, event: Partial<FakeEvent>): Fired {
  const pending: Promise<unknown>[] = [];
  const closed = vi.fn();
  const full: FakeEvent = {
    waitUntil: (work) => {
      pending.push(work);
    },
    ...event,
    notification: event.notification === undefined ? undefined : { ...event.notification, close: closed },
  };
  rig.listeners.get(name)?.(full);
  return { closed, settled: Promise.all(pending) };
}

function pushOf(payload: unknown): Partial<FakeEvent> {
  return { data: { json: () => payload } };
}

function windowAt(url: string): FakeWindow {
  const window: FakeWindow = { url, focus: vi.fn(), navigate: vi.fn() };
  window.focus.mockResolvedValue(window);
  window.navigate.mockResolvedValue(window);
  return window;
}

function clickWith(data: unknown): Partial<FakeEvent> {
  return { notification: { close: () => undefined, data } };
}

describe('push service worker', () => {
  let rig: Rig;

  beforeEach(() => {
    rig = boot();
  });

  it('takes over immediately on install and activation', async () => {
    fire(rig, 'install', {});
    await fire(rig, 'activate', {}).settled;

    expect(rig.skipWaiting).toHaveBeenCalledTimes(1);
    expect(rig.claim).toHaveBeenCalledTimes(1);
  });

  it('shows a generic notification when the payload is malformed', async () => {
    const event = {
      data: {
        json: () => {
          throw new SyntaxError('bad json');
        },
      },
    };
    await fire(rig, 'push', event).settled;

    expect(rig.showNotification).toHaveBeenCalledWith('webhooks.live', { body: 'New capture' });
  });

  it('shows a generic notification when the payload is empty or not an object', async () => {
    await fire(rig, 'push', { data: null }).settled;
    await fire(rig, 'push', pushOf(['x'])).settled;

    expect(rig.showNotification).toHaveBeenNthCalledWith(1, 'webhooks.live', { body: 'New capture' });
    expect(rig.showNotification).toHaveBeenNthCalledWith(2, 'webhooks.live', { body: 'New capture' });
  });

  it('titles the notification with the method and a truncated path as plain text', async () => {
    const path = `/<img src=x onerror=alert(1)>${'a'.repeat(300)}`;
    await fire(rig, 'push', pushOf({ method: 'POST', path })).settled;

    const [title] = rig.showNotification.mock.calls[0] as [string];
    expect(title.startsWith('POST /<img src=x onerror=alert(1)>')).toBe(true);
    expect(Array.from(title).length).toBeLessThanOrEqual('POST '.length + 100 + 1);
    expect(title.endsWith('…')).toBe(true);
  });

  it('collapses repeated pushes for one capture by using the capture id as the tag', async () => {
    const payload = { endpoint_id: ENDPOINT_ID, capture_id: CAPTURE_ID, kind: 'http', method: 'GET', path: '/x' };
    await fire(rig, 'push', pushOf(payload)).settled;

    expect(rig.showNotification).toHaveBeenCalledWith('GET /x', {
      body: 'http',
      tag: CAPTURE_ID,
      data: { endpoint_id: ENDPOINT_ID, capture_id: CAPTURE_ID },
    });
  });

  it('drops ids that fail validation from the notification', async () => {
    const payload = { endpoint_id: 'ZZ', capture_id: '../etc', kind: '<b>', method: 'GET', path: '/x' };
    await fire(rig, 'push', pushOf(payload)).settled;

    expect(rig.showNotification).toHaveBeenCalledWith('GET /x', { body: 'New capture' });
  });

  it('closes the notification and opens the capture in a new window when none is open', async () => {
    const fired = fire(rig, 'notificationclick', clickWith({ endpoint_id: ENDPOINT_ID, capture_id: CAPTURE_ID }));
    await fired.settled;

    expect(fired.closed).toHaveBeenCalledTimes(1);
    expect(rig.openWindow).toHaveBeenCalledWith(NOTIFICATION_TARGET);
  });

  it('focuses an open window of the app and navigates it to the capture', async () => {
    const open = windowAt(`${ORIGIN}/endpoints`);
    const withWindow = boot([windowAt('https://other.example.test/'), open]);
    await fire(withWindow, 'notificationclick', clickWith({ endpoint_id: ENDPOINT_ID, capture_id: CAPTURE_ID }))
      .settled;

    expect(open.focus).toHaveBeenCalledTimes(1);
    expect(open.navigate).toHaveBeenCalledWith(NOTIFICATION_TARGET);
    expect(withWindow.openWindow).not.toHaveBeenCalled();
  });

  it('opens the root instead of a path built from a bad capture id', async () => {
    await fire(rig, 'notificationclick', clickWith({ endpoint_id: ENDPOINT_ID, capture_id: '../../admin' })).settled;

    expect(rig.openWindow).toHaveBeenCalledWith(`${ORIGIN}/`);
  });

  it('opens the root when the notification carries no data', async () => {
    await fire(rig, 'notificationclick', clickWith(null)).settled;

    expect(rig.openWindow).toHaveBeenCalledWith(`${ORIGIN}/`);
  });
});
