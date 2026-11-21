import { describe, expect, it } from 'vitest';
import { isCaptureId, isEndpointId } from './ids';

describe('isEndpointId', () => {
  it('accepts 24 lowercase Crockford base32 characters', () => {
    expect(isEndpointId('0123456789abcdefghjkmnpq')).toBe(true);
    expect(isEndpointId('vwxyzvwxyzvwxyzvwxyzvwxy')).toBe(true);
  });

  it('rejects uppercase, excluded letters, and wrong lengths', () => {
    expect(isEndpointId('0123456789ABCDEFGHJKMNPQ')).toBe(false);
    expect(isEndpointId('0123456789abcdefghjkmnpi')).toBe(false);
    expect(isEndpointId('0123456789abcdefghjkmnp')).toBe(false);
    expect(isEndpointId('0123456789abcdefghjkmnpqr')).toBe(false);
    expect(isEndpointId('')).toBe(false);
  });
});

describe('isCaptureId', () => {
  it('accepts a lowercase hyphenated UUID', () => {
    expect(isCaptureId('0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21')).toBe(true);
  });

  it('rejects uppercase and unhyphenated UUIDs', () => {
    expect(isCaptureId('0193A1C2-7E4B-7C11-9A2F-3B5D1E0F4A21')).toBe(false);
    expect(isCaptureId('0193a1c27e4b7c119a2f3b5d1e0f4a21')).toBe(false);
  });
});
