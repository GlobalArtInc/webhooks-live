import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { AUTO_RUN_DEBOUNCE_MS } from './constants';
import { LatestRunner, type RunnerState } from './runner';

interface Pending {
  request: string;
  signal: AbortSignal;
  resolve: (result: string) => void;
  reject: (failure: unknown) => void;
}

function setup() {
  const pending: Pending[] = [];
  const states: RunnerState<string>[] = [];
  const runner = new LatestRunner<string, string>(
    (request, signal) =>
      new Promise<string>((resolve, reject) => {
        pending.push({ request, signal, resolve, reject });
      }),
    (state) => states.push(state),
  );
  return { runner, pending, last: () => states.at(-1) };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('LatestRunner', () => {
  it('never lets a late response replace a newer run', async () => {
    const { runner, pending, last } = setup();

    runner.runNow('first', 'k1');
    runner.runNow('second', 'k2');
    pending[1]?.resolve('answer to second');
    await vi.advanceTimersByTimeAsync(0);
    pending[0]?.resolve('answer to first');
    await vi.advanceTimersByTimeAsync(0);

    expect(pending[0]?.signal.aborted).toBe(true);
    expect(last()?.result).toBe('answer to second');
    expect(last()?.resultKey).toBe('k2');
    expect(last()?.resultId).toBe(1);
  });

  it('drops a late failure of an aborted run', async () => {
    const { runner, pending, last } = setup();

    runner.runNow('first', 'k1');
    runner.runNow('second', 'k2');
    pending[0]?.reject(new Error('late failure'));
    await vi.advanceTimersByTimeAsync(0);

    expect(last()?.phase).toBe('running');
    expect(last()?.error).toBeNull();
  });

  it('runs once after the debounce with the last scheduled request', () => {
    const { runner, pending, last } = setup();

    runner.schedule('a', 'ka', false);
    runner.schedule('ab', 'kab', false);
    vi.advanceTimersByTime(AUTO_RUN_DEBOUNCE_MS - 1);
    expect(pending).toHaveLength(0);
    expect(last()?.phase).toBe('waiting');

    vi.advanceTimersByTime(1);
    expect(pending).toHaveLength(1);
    expect(pending[0]?.request).toBe('ab');
    expect(last()?.phase).toBe('running');
  });

  it('marks only manual runs as manual and keeps the old result when a run fails', async () => {
    const { runner, pending, last } = setup();

    runner.schedule('auto', 'k1', true);
    pending[0]?.resolve('auto result');
    await vi.advanceTimersByTimeAsync(0);
    expect(last()?.manual).toBe(false);

    runner.runNow('manual', 'k2');
    pending[1]?.reject(new Error('server down'));
    await vi.advanceTimersByTimeAsync(0);

    expect(last()?.phase).toBe('failed');
    expect(last()?.manual).toBe(true);
    expect(last()?.result).toBe('auto result');
  });

  it('cancels a pending debounce without touching a finished result', async () => {
    const { runner, pending, last } = setup();

    runner.schedule('a', 'ka', true);
    pending[0]?.resolve('done');
    await vi.advanceTimersByTimeAsync(0);
    runner.schedule('b', 'kb', false);
    runner.cancelPending();
    vi.advanceTimersByTime(AUTO_RUN_DEBOUNCE_MS * 2);

    expect(pending).toHaveLength(1);
    expect(last()?.phase).toBe('done');
    expect(last()?.result).toBe('done');
  });

  it('ignores everything after dispose', () => {
    const { runner, pending } = setup();

    runner.runNow('a', 'ka');
    runner.dispose();
    runner.runNow('b', 'kb');
    runner.schedule('c', 'kc', true);

    expect(pending).toHaveLength(1);
    expect(pending[0]?.signal.aborted).toBe(true);
  });
});
