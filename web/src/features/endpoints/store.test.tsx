import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { ENDPOINTS_STORAGE_KEY, serializeEndpoints, type MyEndpoint } from './storage';
import {
  forgetEndpoint,
  markEndpointOpened,
  recordEndpointLastSeen,
  recordEndpointTitle,
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

describe('recordEndpointTitle', () => {
  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it('stores the title on a remembered endpoint only, and writes nothing when it is unchanged', () => {
    const { result } = renderHook(() => useMyEndpoints());
    act(() => rememberEndpoint(ENDPOINT_ID));

    act(() => recordEndpointTitle(OTHER_ID, 'Unknown'));
    expect(result.current).toHaveLength(1);

    act(() => recordEndpointTitle(ENDPOINT_ID, 'Stripe test'));
    expect(result.current[0]?.title).toBe('Stripe test');

    const written = window.localStorage.getItem(ENDPOINTS_STORAGE_KEY);
    act(() => recordEndpointTitle(ENDPOINT_ID, 'Stripe test'));
    expect(window.localStorage.getItem(ENDPOINTS_STORAGE_KEY)).toBe(written);

    act(() => recordEndpointTitle(ENDPOINT_ID, null));
    expect(result.current[0]).not.toHaveProperty('title');
  });

  it('remembers the title known at creation', () => {
    const { result } = renderHook(() => useMyEndpoints());
    act(() => rememberEndpoint(ENDPOINT_ID, 'Created title'));
    expect(result.current[0]?.title).toBe('Created title');
  });
});
