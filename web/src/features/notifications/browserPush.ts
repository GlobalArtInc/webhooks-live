import { ApiError } from '../../api/client';
import { subscribePush } from './notificationsApi';

export type PushSupport = 'unsupported' | 'supported';

export type PushOutcome = 'subscribed' | 'denied' | 'worker-failed';

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

async function waitForWorker(): Promise<ServiceWorkerRegistration | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<null>((resolve) => {
    timer = setTimeout(() => resolve(null), WORKER_READY_TIMEOUT_MS);
  });
  try {
    await navigator.serviceWorker.register(WORKER_URL, { scope: WORKER_SCOPE });
    return await Promise.race([navigator.serviceWorker.ready, timeout]);
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
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

export async function subscribeThisBrowser(publicKey: string): Promise<PushOutcome> {
  const permission = await Notification.requestPermission();
  if (permission !== 'granted') {
    return 'denied';
  }
  const registration = await waitForWorker();
  if (registration === null) {
    return 'worker-failed';
  }
  const existing = await registration.pushManager.getSubscription();
  const subscription =
    existing ??
    (await registration.pushManager.subscribe({
      userVisibleOnly: true,
      applicationServerKey: base64UrlToBytes(publicKey),
    }));
  await saveSubscription(subscription);
  return 'subscribed';
}
