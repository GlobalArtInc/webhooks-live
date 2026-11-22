import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from 'vitest';
import { ApiError } from '../../api/client';
import {
  base64UrlToBytes,
  type PublicKeyLoader,
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
  options: { applicationServerKey: ArrayBuffer | null };
  unsubscribe: ReturnType<typeof vi.fn>;
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

function keyBuffer(key: string): ArrayBuffer {
  return base64UrlToBytes(key).buffer;
}

function fakeSubscription(key: string = PUBLIC_KEY): FakeSubscription {
  return {
    endpoint: ENDPOINT_URL,
    options: { applicationServerKey: keyBuffer(key) },
    unsubscribe: vi.fn().mockResolvedValue(true),
    toJSON: () => ({ keys: { p256dh: 'key-p256dh', auth: 'key-auth' } }),
  };
}

function newPushManager(existing: FakeSubscription | null): FakePushManager {
  return {
    getSubscription: vi.fn().mockResolvedValue(existing),
    subscribe: vi.fn().mockResolvedValue(fakeSubscription()),
  };
}

function installWorker(
  push: FakePushManager,
  ready: Promise<unknown>,
  register = vi.fn().mockResolvedValue({}),
): FakeWorkerRig {
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

function allowPermission(result: NotificationPermission): ReturnType<typeof vi.fn> {
  const requestPermission = vi.fn().mockResolvedValue(result);
  vi.stubGlobal('Notification', { requestPermission });
  return requestPermission;
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
  let loadKey: Mock<PublicKeyLoader>;

  beforeEach(() => {
    loadKey = vi.fn<PublicKeyLoader>().mockResolvedValue(PUBLIC_KEY);
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

    expect(await subscribeThisBrowser(loadKey)).toBe('subscribed');

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

  it('asks for the permission before anything else, in the same tick as the call', () => {
    const requestPermission = allowPermission('granted');
    installReadyWorker();

    void subscribeThisBrowser(loadKey);

    expect(requestPermission).toHaveBeenCalledTimes(1);
    expect(loadKey).not.toHaveBeenCalled();
  });

  it('does not fetch the key or touch the worker when the user blocks the permission', async () => {
    allowPermission('denied');
    const rig = installReadyWorker();

    expect(await subscribeThisBrowser(loadKey)).toBe('denied');

    expect(loadKey).not.toHaveBeenCalled();
    expect(rig.register).not.toHaveBeenCalled();
    expect(subscribePush).not.toHaveBeenCalled();
  });

  it('reports a dismissed prompt separately from a block', async () => {
    allowPermission('default');
    const rig = installReadyWorker();

    expect(await subscribeThisBrowser(loadKey)).toBe('dismissed');

    expect(rig.register).not.toHaveBeenCalled();
  });

  it('reports a worker failure when the registration is rejected', async () => {
    const rig = installReadyWorker();
    rig.register.mockRejectedValue(new TypeError('script failed'));

    expect(await subscribeThisBrowser(loadKey)).toBe('worker-failed');

    expect(rig.push.subscribe).not.toHaveBeenCalled();
    expect(subscribePush).not.toHaveBeenCalled();
  });

  it('reports a worker failure when the worker never becomes ready', async () => {
    vi.useFakeTimers();
    const rig = installWorker(newPushManager(null), new Promise(() => undefined));

    const outcome = subscribeThisBrowser(loadKey);
    await vi.advanceTimersByTimeAsync(WORKER_READY_TIMEOUT_MS);

    expect(await outcome).toBe('worker-failed');
    expect(rig.push.subscribe).not.toHaveBeenCalled();
  });

  it('reports a worker failure when the registration never settles', async () => {
    vi.useFakeTimers();
    const push = newPushManager(null);
    const register = vi.fn().mockReturnValue(new Promise(() => undefined));
    installWorker(push, Promise.resolve({ pushManager: push }), register);

    const outcome = subscribeThisBrowser(loadKey);
    await vi.advanceTimersByTimeAsync(WORKER_READY_TIMEOUT_MS);

    expect(await outcome).toBe('worker-failed');
    expect(push.subscribe).not.toHaveBeenCalled();
  });

  it('reuses a browser subscription that already exists', async () => {
    const rig = installReadyWorker(fakeSubscription());

    expect(await subscribeThisBrowser(loadKey)).toBe('subscribed');

    expect(rig.push.subscribe).not.toHaveBeenCalled();
    expect(subscribePush).toHaveBeenCalledTimes(1);
  });

  it('replaces an existing subscription made with a different server key', async () => {
    const stale = fakeSubscription('AAAA');
    const rig = installReadyWorker(stale);

    expect(await subscribeThisBrowser(loadKey)).toBe('subscribed');

    expect(stale.unsubscribe).toHaveBeenCalledTimes(1);
    expect(rig.push.subscribe).toHaveBeenCalledTimes(1);
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

    expect(await subscribeThisBrowser(loadKey)).toBe('subscribed');
  });

  it('lets other server errors reach the caller', async () => {
    installReadyWorker();
    vi.mocked(subscribePush).mockRejectedValue(
      new ApiError({ status: 503, code: 'feature_disabled', message: 'off', details: [], retryAfter: null }),
    );

    await expect(subscribeThisBrowser(loadKey)).rejects.toBeInstanceOf(ApiError);
  });
});
