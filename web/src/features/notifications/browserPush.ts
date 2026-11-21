import { subscribePush } from './notificationsApi';

export type PushSupport = 'unsupported' | 'supported';

export type PushOutcome = 'subscribed' | 'denied' | 'needs-app-worker';

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

export async function subscribeThisBrowser(publicKey: string): Promise<PushOutcome> {
  const permission = await Notification.requestPermission();
  if (permission !== 'granted') {
    return 'denied';
  }
  const registration = await navigator.serviceWorker.getRegistration();
  if (registration === undefined) {
    return 'needs-app-worker';
  }
  const subscription = await registration.pushManager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: base64UrlToBytes(publicKey),
  });
  const json = subscription.toJSON();
  const p256dh = json.keys?.p256dh ?? '';
  const auth = json.keys?.auth ?? '';
  await subscribePush({
    endpoint: subscription.endpoint,
    keys: { p256dh, auth },
    user_agent: navigator.userAgent,
  });
  return 'subscribed';
}
