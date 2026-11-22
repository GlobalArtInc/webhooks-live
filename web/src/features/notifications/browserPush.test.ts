import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '../../api/client';
import {
  base64UrlToBytes,
  pushSupportOf,
  subscribeThisBrowser,
  WORKER_READY_TIMEOUT_MS,
  WORKER_SCOPE,
  WORKER_URL,
} from './browserPush';
import { subscribePush } from './notificationsApi';

vi.mock('./notificationsApi', () => ({ subscribePush: vi.fn() }));

const ENDPOINT_URL = 'https://fcm.googleapis.com/fcm/send/abc';
const PUBLIC_KEY = '-_8';

interface FakeSubscription {
  endpoint: string;
  toJSON: () => { keys: { p256dh: string; auth: string } };
}

interface FakePushManager {
  getSubscription: ReturnType<typeof vi.fn>;
  subscribe: ReturnType<typeof vi.fn>;
}

interface FakeWorkerRig {
  register: ReturnType<typeof vi.fn>;
  push: FakePushManager;
}

function fakeSubscription(): FakeSubscription {
  return { endpoint: ENDPOINT_URL, toJSON: () => ({ keys: { p256dh: 'key-p256dh', auth: 'key-auth' } }) };
}

function newPushManager(existing: FakeSubscription | null): FakePushManager {
  return {
    getSubscription: vi.fn().mockResolvedValue(existing),
    subscribe: vi.fn().mockResolvedValue(fakeSubscription()),
  };
}

function installWorker(push: FakePushManager, ready: Promise<unknown>): FakeWorkerRig {
  const register = vi.fn().mockResolvedValue({});
  Object.defineProperty(navigator, 'serviceWorker', {
    configurable: true,
    value: { register, ready },
  });
  return { register, push };
}

function installReadyWorker(existing: FakeSubscription | null = null): FakeWorkerRig {
  const push = newPushManager(existing);
  return installWorker(push, Promise.resolve({ pushManager: push }));
}

function allowPermission(result: NotificationPermission): void {
  vi.stubGlobal('Notification', { requestPermission: vi.fn().mockResolvedValue(result) });
}

describe('browser push support', () => {
  it('is unsupported when any of the three browser APIs is missing', () => {
    expect(pushSupportOf({ hasNotification: true, hasServiceWorker: true, hasPushManager: false })).toBe('unsupported');
    expect(pushSupportOf({ hasNotification: true, hasServiceWorker: true, hasPushManager: true })).toBe('supported');
  });

  it('decodes a base64url application server key without padding', () => {
    expect(Array.from(base64UrlToBytes('-_8'))).toEqual([251, 255]);
  });
});

describe('subscribing this browser', () => {
  beforeEach(() => {
    vi.mocked(subscribePush).mockReset();
    vi.mocked(subscribePush).mockResolvedValue({ id: 'sub-1', created_at: '2026-11-22T10:00:00.000Z' });
    allowPermission('granted');
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    Reflect.deleteProperty(navigator, 'serviceWorker');
  });

  it('registers the worker at the site scope, waits for it and sends the subscription to the server', async () => {
    const rig = installReadyWorker();

    expect(await subscribeThisBrowser(PUBLIC_KEY)).toBe('subscribed');

    expect(rig.register).toHaveBeenCalledWith(WORKER_URL, { scope: WORKER_SCOPE });
    expect(rig.push.subscribe).toHaveBeenCalledWith({
      userVisibleOnly: true,
      applicationServerKey: base64UrlToBytes(PUBLIC_KEY),
    });
    expect(subscribePush).toHaveBeenCalledWith({
      endpoint: ENDPOINT_URL,
      keys: { p256dh: 'key-p256dh', auth: 'key-auth' },
      user_agent: navigator.userAgent,
    });
  });

  it('does not touch the worker when the user denies the permission', async () => {
    allowPermission('denied');
    const rig = installReadyWorker();

    expect(await subscribeThisBrowser(PUBLIC_KEY)).toBe('denied');

    expect(rig.register).not.toHaveBeenCalled();
    expect(subscribePush).not.toHaveBeenCalled();
  });

  it('reports a worker failure when the registration is rejected', async () => {
    const rig = installReadyWorker();
    rig.register.mockRejectedValue(new TypeError('script failed'));

    expect(await subscribeThisBrowser(PUBLIC_KEY)).toBe('worker-failed');

    expect(rig.push.subscribe).not.toHaveBeenCalled();
    expect(subscribePush).not.toHaveBeenCalled();
  });

  it('reports a worker failure when the worker never becomes ready', async () => {
    vi.useFakeTimers();
    const rig = installWorker(newPushManager(null), new Promise(() => undefined));

    const outcome = subscribeThisBrowser(PUBLIC_KEY);
    await vi.advanceTimersByTimeAsync(WORKER_READY_TIMEOUT_MS);

    expect(await outcome).toBe('worker-failed');
    expect(rig.push.subscribe).not.toHaveBeenCalled();
  });

  it('reuses a browser subscription that already exists', async () => {
    const rig = installReadyWorker(fakeSubscription());

    expect(await subscribeThisBrowser(PUBLIC_KEY)).toBe('subscribed');

    expect(rig.push.subscribe).not.toHaveBeenCalled();
    expect(subscribePush).toHaveBeenCalledTimes(1);
  });

  it('treats a subscription the server already knows as subscribed', async () => {
    installReadyWorker(fakeSubscription());
    vi.mocked(subscribePush).mockRejectedValue(
      new ApiError({
        status: 422,
        code: 'already_registered',
        message: 'this push subscription is already registered',
        details: [],
        retryAfter: null,
      }),
    );

    expect(await subscribeThisBrowser(PUBLIC_KEY)).toBe('subscribed');
  });

  it('lets other server errors reach the caller', async () => {
    installReadyWorker();
    vi.mocked(subscribePush).mockRejectedValue(
      new ApiError({ status: 503, code: 'feature_disabled', message: 'off', details: [], retryAfter: null }),
    );

    await expect(subscribeThisBrowser(PUBLIC_KEY)).rejects.toBeInstanceOf(ApiError);
  });
});
