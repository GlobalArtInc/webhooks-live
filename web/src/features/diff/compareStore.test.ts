import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { clearMarkedCapture, compareWith, getMarkedCapture, markCapture, useMarkedCapture } from './compareStore';

const ENDPOINT_ID = '0123456789abcdefghjkmnpq';
const OTHER_ENDPOINT_ID = '0123456789abcdefghjkmnpr';
const FIRST_CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21';
const SECOND_CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a22';

describe('compare store', () => {
  afterEach(() => {
    cleanup();
    clearMarkedCapture(ENDPOINT_ID);
    clearMarkedCapture(OTHER_ENDPOINT_ID);
  });

  it('remembers the marked capture for an endpoint', () => {
    markCapture(ENDPOINT_ID, FIRST_CAPTURE_ID);

    expect(getMarkedCapture(ENDPOINT_ID)).toBe(FIRST_CAPTURE_ID);
  });

  it('returns route params that compare the marked capture with the given one', () => {
    markCapture(ENDPOINT_ID, FIRST_CAPTURE_ID);

    expect(compareWith(ENDPOINT_ID, SECOND_CAPTURE_ID)).toEqual({
      id: ENDPOINT_ID,
      a: FIRST_CAPTURE_ID,
      b: SECOND_CAPTURE_ID,
    });
  });

  it('returns null from compareWith when nothing is marked for the endpoint', () => {
    expect(compareWith(ENDPOINT_ID, SECOND_CAPTURE_ID)).toBeNull();
  });

  it('clears the mark for one endpoint without touching another', () => {
    markCapture(ENDPOINT_ID, FIRST_CAPTURE_ID);
    markCapture(OTHER_ENDPOINT_ID, SECOND_CAPTURE_ID);

    clearMarkedCapture(ENDPOINT_ID);

    expect(getMarkedCapture(ENDPOINT_ID)).toBeNull();
    expect(getMarkedCapture(OTHER_ENDPOINT_ID)).toBe(SECOND_CAPTURE_ID);
  });

  it('notifies hook subscribers when the mark changes', () => {
    const { result } = renderHook(() => useMarkedCapture(ENDPOINT_ID));
    expect(result.current).toBeNull();

    act(() => {
      markCapture(ENDPOINT_ID, FIRST_CAPTURE_ID);
    });
    expect(result.current).toBe(FIRST_CAPTURE_ID);

    act(() => {
      clearMarkedCapture(ENDPOINT_ID);
    });
    expect(result.current).toBeNull();
  });
});
