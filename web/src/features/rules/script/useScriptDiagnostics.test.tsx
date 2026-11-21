import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { VALIDATE_DEBOUNCE_MS } from './constants';
import type { ScriptDiagnostics, ScriptTransport } from './types';
import { useScriptDiagnostics } from './useScriptDiagnostics';

interface PendingCall {
  source: string;
  signal: AbortSignal;
  resolve: (diagnostics: ScriptDiagnostics) => void;
  reject: (error: Error) => void;
}

interface FakeServer {
  transport: ScriptTransport;
  calls: PendingCall[];
}

function problem(line: number): ScriptDiagnostics {
  return [{ line, column: 1, length: 1, code: 'script_invalid', message: `Problem ${line}`, severity: 'error' }];
}

function fakeServer(): FakeServer {
  const calls: PendingCall[] = [];
  const transport: ScriptTransport = {
    id: 'fake',
    loadMeta: () => Promise.reject(new Error('unused')),
    validate: (source, signal) =>
      new Promise<ScriptDiagnostics>((resolve, reject) => {
        calls.push({ source, signal, resolve, reject });
      }),
  };
  return { transport, calls };
}

async function settle(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
  });
}

async function advance(ms: number): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('useScriptDiagnostics', () => {
  it('waits for the debounce, asks once, and reports the answer', async () => {
    const server = fakeServer();
    const { result, rerender } = renderHook(
      ({ source }) => useScriptDiagnostics(source, { transport: server.transport }),
      {
        initialProps: { source: 'let a = 1' },
      },
    );
    expect(result.current.state).toBe('pending');

    await advance(VALIDATE_DEBOUNCE_MS - 50);
    rerender({ source: 'let a = 12' });
    await advance(VALIDATE_DEBOUNCE_MS - 50);
    expect(server.calls).toHaveLength(0);

    await advance(60);
    expect(server.calls.map((call) => call.source)).toEqual(['let a = 12']);

    await act(async () => {
      server.calls[0]?.resolve(problem(1));
      await Promise.resolve();
    });
    expect(result.current.state).toBe('ready');
    expect(result.current.diagnostics).toEqual(problem(1));
  });

  it('aborts the request in flight when the text changes and ignores its late answer', async () => {
    const server = fakeServer();
    const { result, rerender } = renderHook(
      ({ source }) => useScriptDiagnostics(source, { transport: server.transport }),
      {
        initialProps: { source: 'first' },
      },
    );
    await advance(VALIDATE_DEBOUNCE_MS);
    const first = server.calls[0];
    expect(first?.signal.aborted).toBe(false);

    rerender({ source: 'second' });
    expect(first?.signal.aborted).toBe(true);
    await advance(VALIDATE_DEBOUNCE_MS);
    expect(server.calls).toHaveLength(2);

    await act(async () => {
      server.calls[1]?.resolve([]);
      await Promise.resolve();
    });
    await act(async () => {
      first?.resolve(problem(7));
      await Promise.resolve();
    });
    await settle();
    expect(result.current.state).toBe('ready');
    expect(result.current.diagnostics).toEqual([]);
  });

  it('does not ask for a blank script, an oversize script, or when disabled', async () => {
    const server = fakeServer();
    const blank = renderHook(() => useScriptDiagnostics('  \n', { transport: server.transport }));
    const oversize = renderHook(() => useScriptDiagnostics('abcdef', { transport: server.transport, maxBytes: 5 }));
    const off = renderHook(() => useScriptDiagnostics('let a = 1', { transport: server.transport, enabled: false }));
    await advance(VALIDATE_DEBOUNCE_MS * 2);

    expect(server.calls).toHaveLength(0);
    expect(blank.result.current.state).toBe('ready');
    expect(oversize.result.current.state).toBe('ready');
    expect(off.result.current.state).toBe('idle');
  });

  it('reports a failed check with no problems and asks again on retry', async () => {
    const server = fakeServer();
    const { result } = renderHook(() => useScriptDiagnostics('let a = 1', { transport: server.transport }));
    await advance(VALIDATE_DEBOUNCE_MS);
    await act(async () => {
      server.calls[0]?.reject(new Error('offline'));
      await Promise.resolve();
    });
    expect(result.current.state).toBe('failed');
    expect(result.current.diagnostics).toEqual([]);

    act(() => {
      result.current.retry();
    });
    expect(result.current.state).toBe('pending');
    await advance(VALIDATE_DEBOUNCE_MS);
    expect(server.calls).toHaveLength(2);
    await act(async () => {
      server.calls[1]?.resolve(problem(3));
      await Promise.resolve();
    });
    expect(result.current.state).toBe('ready');
    expect(result.current.diagnostics).toEqual(problem(3));
  });

  it('aborts the pending request on unmount', async () => {
    const server = fakeServer();
    const { unmount } = renderHook(() => useScriptDiagnostics('let a = 1', { transport: server.transport }));
    await advance(VALIDATE_DEBOUNCE_MS);
    unmount();
    expect(server.calls[0]?.signal.aborted).toBe(true);
  });
});
