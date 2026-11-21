import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { rememberEndpoint, useMyEndpoints } from './store';

const ENDPOINT_ID = '0123456789abcdefghjkmnpq';

describe('useMyEndpoints without usable storage', () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it('reads as empty when storage access throws', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    const { result } = renderHook(() => useMyEndpoints());

    expect(result.current).toEqual([]);
  });

  it('keeps the list in memory for the session when writes are refused', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('quota exceeded', 'QuotaExceededError');
    });
    const { result } = renderHook(() => useMyEndpoints());

    act(() => rememberEndpoint(ENDPOINT_ID));

    expect(result.current.map((item) => item.id)).toEqual([ENDPOINT_ID]);
  });
});
