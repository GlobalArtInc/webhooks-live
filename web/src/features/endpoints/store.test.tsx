import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints, type MyEndpoint } from './storage';
import {
  forgetEndpoint,
  markEndpointOpened,
  recordEndpointLastSeen,
  rememberEndpoint,
  renameMyEndpoint,
  useMyEndpoints,
} from './store';

const ENDPOINT_ID = '0123456789abcdefghjkmnpq';
const OTHER_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';
const CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21';

function storedEntry(id: string): MyEndpoint {
  return { id, label: null, added: 1, lastOpened: 1, lastSeenCaptureId: null };
}

describe('useMyEndpoints', () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it('reflects remember, rename, touch, last seen and forget', () => {
    const { result } = renderHook(() => useMyEndpoints());
    expect(result.current).toEqual([]);

    act(() => rememberEndpoint(ENDPOINT_ID));
    expect(result.current.map((item) => item.id)).toEqual([ENDPOINT_ID]);

    act(() => renameMyEndpoint(ENDPOINT_ID, 'stripe-dev'));
    expect(result.current[0]?.label).toBe('stripe-dev');

    act(() => markEndpointOpened(ENDPOINT_ID));
    act(() => recordEndpointLastSeen(ENDPOINT_ID, CAPTURE_ID));
    expect(result.current[0]?.lastSeenCaptureId).toBe(CAPTURE_ID);

    act(() => forgetEndpoint(ENDPOINT_ID));
    expect(result.current).toEqual([]);
  });

  it('updates when another tab writes the key and fires a storage event', () => {
    const { result } = renderHook(() => useMyEndpoints());
    const raw = serializeEndpoints([storedEntry(OTHER_ID)]);

    act(() => {
      window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, raw);
      window.dispatchEvent(new StorageEvent('storage', { key: ENDPOINTS_STORAGE_KEY, newValue: raw }));
    });

    expect(result.current.map((item) => item.id)).toEqual([OTHER_ID]);
  });

  it('shows an empty list when the stored data is unreadable', () => {
    window.localStorage.setItem(ENDPOINTS_STORAGE_KEY, '{"version":1,"endpoints":');
    const { result } = renderHook(() => useMyEndpoints());

    expect(result.current).toEqual([]);
    act(() => rememberEndpoint(ENDPOINT_ID));
    expect(result.current.map((item) => item.id)).toEqual([ENDPOINT_ID]);
  });
});
