import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PREVIEW_DEBOUNCE_MS, PreviewRunner } from './previewRunner';
import type { PreviewRequest, PreviewResult, PreviewState, RenderedResponse } from './types';

interface Pending {
  request: PreviewRequest;
  signal: AbortSignal;
  resolve: (result: PreviewResult) => void;
  reject: (failure: unknown) => void;
}

function requestOf(body: string): PreviewRequest {
  return {
    capture_id: 'capture-1',
    response: { status: 200, content_type: 'text/plain', headers: [], body: { encoding: 'utf8', data: body } },
  };
}

function renderedOf(body: string): RenderedResponse {
  return {
    status: 200,
    content_type: 'text/plain',
    headers: [],
    body: { encoding: 'utf8', data: body, truncated: false },
    body_size: body.length,
    duration_ms: 1,
  };
}

function resultOf(body: string | null): PreviewResult {
  return {
    capture_id: 'capture-1',
    rendered: body === null ? null : renderedOf(body),
    diagnostics: [],
  };
}

function setup() {
  const pending: Pending[] = [];
  const states: PreviewState[] = [];
  const runner = new PreviewRunner(
    (request, signal) =>
      new Promise<PreviewResult>((resolve, reject) => {
        pending.push({ request, signal, resolve, reject });
      }),
    (state) => states.push(state),
  );
  return { runner, pending, states, last: () => states.at(-1) };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe('PreviewRunner', () => {
  it('renders once after the debounce, using the last scheduled request', () => {
    const { runner, pending, last } = setup();

    runner.schedule(requestOf('a'), 'a', false);
    runner.schedule(requestOf('ab'), 'ab', false);
    vi.advanceTimersByTime(PREVIEW_DEBOUNCE_MS - 1);
    expect(pending).toHaveLength(0);
    expect(last()?.phase).toBe('waiting');

    vi.advanceTimersByTime(1);
    expect(pending).toHaveLength(1);
    expect(pending[0]?.request.response.body.data).toBe('ab');
    expect(last()?.phase).toBe('rendering');
  });

  it('renders at once when asked to, and aborts the render it replaces', () => {
    const { runner, pending } = setup();

    runner.schedule(requestOf('a'), 'a', true);
    runner.runNow(requestOf('b'), 'b');

    expect(pending).toHaveLength(2);
    expect(pending[0]?.signal.aborted).toBe(true);
    expect(pending[1]?.signal.aborted).toBe(false);
  });

  it('never lets an older response replace a newer one', async () => {
    const { runner, pending, last } = setup();

    runner.runNow(requestOf('old'), 'old');
    runner.runNow(requestOf('new'), 'new');
    pending[1]?.resolve(resultOf('new body'));
    await vi.advanceTimersByTimeAsync(0);
    pending[0]?.resolve(resultOf('old body'));
    await vi.advanceTimersByTimeAsync(0);

    expect(last()?.result?.rendered?.body.data).toBe('new body');
    expect(last()?.resultKey).toBe('new');
  });

  it('ignores the failure of a render that was superseded', async () => {
    const { runner, pending, last } = setup();

    runner.runNow(requestOf('old'), 'old');
    runner.runNow(requestOf('new'), 'new');
    pending[0]?.reject(new DOMException('aborted', 'AbortError'));
    await vi.advanceTimersByTimeAsync(0);

    expect(last()?.phase).toBe('rendering');
    expect(last()?.error).toBeNull();
  });

  it('keeps the last valid render when a later result has errors, and when a render fails', async () => {
    const { runner, pending, last } = setup();

    runner.runNow(requestOf('a'), 'a');
    pending[0]?.resolve(resultOf('first'));
    await vi.advanceTimersByTimeAsync(0);
    runner.runNow(requestOf('b'), 'b');
    pending[1]?.resolve(resultOf(null));
    await vi.advanceTimersByTimeAsync(0);

    expect(last()?.result?.rendered).toBeNull();
    expect(last()?.lastRendered?.body.data).toBe('first');

    runner.runNow(requestOf('c'), 'c');
    const failure = new Error('server down');
    pending[2]?.reject(failure);
    await vi.advanceTimersByTimeAsync(0);

    expect(last()?.phase).toBe('failed');
    expect(last()?.error).toBe(failure);
    expect(last()?.lastRendered?.body.data).toBe('first');
  });

  it('does nothing after dispose', async () => {
    const { runner, pending, states } = setup();

    runner.schedule(requestOf('a'), 'a', false);
    runner.dispose();
    vi.advanceTimersByTime(PREVIEW_DEBOUNCE_MS * 2);
    runner.runNow(requestOf('b'), 'b');

    expect(pending).toHaveLength(0);
    expect(states.at(-1)?.phase).toBe('waiting');
  });
});
