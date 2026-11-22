import { ApiError } from '../../api/client';
import { subscribePush } from './notificationsApi';

export type PushSupport = 'unsupported' | 'supported';

export type PushOutcome = 'subscribed' | 'denied' | 'dismissed' | 'worker-failed';

export type PublicKeyLoader = () => Promise<string>;

export const WORKER_URL = '/sw.js';
export const WORKER_SCOPE = '/';
export const WORKER_READY_TIMEOUT_MS = 10_000;
const ALREADY_REGISTERED = 'already_registered';

export interface PushEnvironment {
  hasNotification: boolean;
  hasServiceWorker: boolean;
  hasPushManager: boolean;
}

export function pushSupportOf(environment: PushEnvironment): PushSupport {
  return environment.hasNotification && environment.hasServiceWorker && environment.hasPushManager
    ? 'supported'
    : 'unsupported';
}

export function currentPushEnvironment(): PushEnvironment {
  return {
    hasNotification: 'Notification' in window,
    hasServiceWorker: 'serviceWorker' in navigator,
    hasPushManager: 'PushManager' in window,
  };
}

export function base64UrlToBytes(value: string): Uint8Array<ArrayBuffer> {
  const padded = value
    .replace(/-/g, '+')
    .replace(/_/g, '/')
    .padEnd(Math.ceil(value.length / 4) * 4, '=');
  const binary = atob(padded);
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

async function startWorker(): Promise<ServiceWorkerRegistration> {
  await navigator.serviceWorker.register(WORKER_URL, { scope: WORKER_SCOPE });
  return navigator.serviceWorker.ready;
}

async function waitForWorker(): Promise<ServiceWorkerRegistration | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>((resolve) => {
    timer = setTimeout(() => resolve(null), WORKER_READY_TIMEOUT_MS);
  });
  try {
    return await Promise.race([startWorker(), timeout]);
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

function sameKey(subscription: PushSubscription, key: Uint8Array): boolean {
  const current = subscription.options.applicationServerKey;
  if (current === null) {
    return false;
  }
  const bytes = new Uint8Array(current);
  return bytes.length === key.length && bytes.every((value, index) => value === key[index]);
}

async function saveSubscription(subscription: PushSubscription): Promise<void> {
  const json = subscription.toJSON();
  const p256dh = json.keys?.p256dh ?? '';
  const auth = json.keys?.auth ?? '';
  try {
    await subscribePush({
      endpoint: subscription.endpoint,
      keys: { p256dh, auth },
      user_agent: navigator.userAgent,
    });
  } catch (error) {
    if (!(error instanceof ApiError && error.code === ALREADY_REGISTERED)) {
      throw error;
    }
  }
}

export async function subscribeThisBrowser(loadPublicKey: PublicKeyLoader): Promise<PushOutcome> {
  const permission = await Notification.requestPermission();
  if (permission === 'default') {
    return 'dismissed';
  }
  if (permission !== 'granted') {
    return 'denied';
  }
  const key = base64UrlToBytes(await loadPublicKey());
  const registration = await waitForWorker();
  if (registration === null) {
    return 'worker-failed';
  }
  let subscription = await registration.pushManager.getSubscription();
  if (subscription !== null && !sameKey(subscription, key)) {
    await subscription.unsubscribe();
    subscription = null;
  }
  const active =
    subscription ?? (await registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key }));
  await saveSubscription(active);
  return 'subscribed';
}
