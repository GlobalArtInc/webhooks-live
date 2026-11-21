import { describe, expect, it } from 'vitest';
import { base64UrlToBytes, pushSupportOf } from './browserPush';

describe('browser push support', () => {
  it('is unsupported when any of the three browser APIs is missing', () => {
    expect(pushSupportOf({ hasNotification: true, hasServiceWorker: true, hasPushManager: false })).toBe('unsupported');
    expect(pushSupportOf({ hasNotification: true, hasServiceWorker: true, hasPushManager: true })).toBe('supported');
  });

  it('decodes a base64url application server key without padding', () => {
    expect(Array.from(base64UrlToBytes('-_8'))).toEqual([251, 255]);
  });
});
