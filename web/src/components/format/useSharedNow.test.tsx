import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FAST_TICK_MS, latestTimestamp, SLOW_TICK_MS, sharedSubscriberCount, useSharedNow } from './useSharedNow';

const START = Date.parse('2026-11-22T10:00:00.000Z');

function setHidden(value: boolean): void {
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => value });
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('useSharedNow', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(START);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    Reflect.deleteProperty(document, 'hidden');
  });

  it('ticks every second while a shown time is under a minute old, then every fifteen seconds', () => {
    const { result } = renderHook(() => useSharedNow(START - 54_000));
    expect(result.current).toBe(START);

    act(() => {
      vi.advanceTimersByTime(FAST_TICK_MS);
    });
    expect(result.current).toBe(START + FAST_TICK_MS);

    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(result.current).toBe(START + 6_000);

    act(() => {
      vi.advanceTimersByTime(SLOW_TICK_MS - 1);
    });
    expect(result.current).toBe(START + 6_000);
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(result.current).toBe(START + 6_000 + SLOW_TICK_MS);
  });

  it('uses the slow cadence when nothing fresh is shown, with a single timer for all consumers', () => {
    const first = renderHook(() => useSharedNow(START - 3_600_000));
    const second = renderHook(() => useSharedNow());
    const third = renderHook(() => useSharedNow(null));

    expect(sharedSubscriberCount()).toBe(3);
    expect(vi.getTimerCount()).toBe(1);
    act(() => {
      vi.advanceTimersByTime(SLOW_TICK_MS);
    });
    expect(first.result.current).toBe(START + SLOW_TICK_MS);
    expect(second.result.current).toBe(START + SLOW_TICK_MS);
    expect(third.result.current).toBe(START + SLOW_TICK_MS);
  });

  it('switches to the fast cadence when a fresh time appears', () => {
    const { result, rerender } = renderHook(({ freshest }) => useSharedNow(freshest), {
      initialProps: { freshest: null as number | null },
    });
    act(() => {
      vi.advanceTimersByTime(SLOW_TICK_MS);
    });
    const before = result.current;

    rerender({ freshest: before - 5_000 });
    const afterRerender = result.current;
    act(() => {
      vi.advanceTimersByTime(FAST_TICK_MS);
    });
    expect(result.current).toBe(afterRerender + FAST_TICK_MS);
  });

  it('pauses while the document is hidden, refreshes on return, and stops with the last consumer', () => {
    const { result, unmount } = renderHook(() => useSharedNow(START));
    setHidden(true);
    expect(vi.getTimerCount()).toBe(0);
    act(() => {
      vi.advanceTimersByTime(120_000);
    });
    expect(result.current).toBe(START);

    act(() => {
      setHidden(false);
    });
    expect(result.current).toBe(START + 120_000);
    expect(vi.getTimerCount()).toBe(1);

    unmount();
    expect(sharedSubscriberCount()).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('finds the latest timestamp among strings and numbers and ignores unreadable ones', () => {
    expect(latestTimestamp([])).toBeNull();
    expect(latestTimestamp(['nope'])).toBeNull();
    expect(latestTimestamp(['2026-11-22T10:00:00.000Z', START + 5, 'nope'])).toBe(START + 5);
  });
});
