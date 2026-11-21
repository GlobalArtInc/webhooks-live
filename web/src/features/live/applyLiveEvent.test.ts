import { describe, expect, it } from 'vitest';
import { queryKeys } from '../../api/queries';
import type { Capture } from '../../api/types';
import { captureSeries } from '../endpoint/testFixtures';
import {
  applyLiveEvent,
  applyLiveEventToCache,
  matchesActiveFilter,
  worstForwardState,
  type LiveCacheState,
} from './applyLiveEvent';
import {
  cachedEndpoint,
  cachedList,
  createSeededClient,
  endpointWith,
  ENDPOINT_ID,
  jobFor,
  listOf,
  seqsOf,
  summaryFor,
} from './liveFixtures';
import { isLiveEventName, parseLiveEvent, readLiveEvent } from './parse';
import type { LiveEvent } from './types';

function stateWith(seqs: readonly number[], overrides: Partial<LiveCacheState> = {}): LiveCacheState {
  return {
    pages: listOf(seqs),
    order: 'desc',
    endpoint: endpointWith({ capture_count: seqs.length, last_seq: Math.max(0, ...seqs) }),
    detail: undefined,
    ...overrides,
  };
}

function captureEvent(seq: number): LiveEvent {
  return { name: 'capture', data: summaryFor(seq) };
}

describe('applyLiveEvent', () => {
  it('inserts a new capture at the top of the first page and updates the counters', () => {
    const outcome = applyLiveEvent(stateWith([3, 2, 1]), captureEvent(4));

    expect(seqsOf(outcome.pages)).toEqual([4, 3, 2, 1]);
    expect(outcome.pages?.pages[0]?.total).toBe(4);
    expect(outcome.endpoint?.capture_count).toBe(4);
    expect(outcome.endpoint?.last_seq).toBe(4);
    expect(outcome.endpoint?.bytes_used).toBe(4);
    expect(outcome.effects).toEqual([{ kind: 'inserted', capture: summaryFor(4) }]);
  });

  it('keeps the page cursors and the page count untouched when inserting', () => {
    const state = stateWith([6, 5, 4, 3, 2, 1], {
      pages: listOf([6, 5, 4, 3, 2, 1], { pageSize: 3, nextCursor: 'more' }),
    });
    const outcome = applyLiveEvent(state, captureEvent(7));

    expect(outcome.pages?.pages.map((page) => page.next_cursor)).toEqual(['3', 'more']);
    expect(outcome.pages?.pages.map((page) => page.items.length)).toEqual([4, 3]);
    expect(outcome.pages?.pageParams).toEqual(state.pages?.pageParams);
  });

  it('treats a duplicate by id as harmless', () => {
    const state = stateWith([3, 2, 1]);
    const outcome = applyLiveEvent(state, captureEvent(3));

    expect(outcome.pages).toBe(state.pages);
    expect(outcome.endpoint).toBe(state.endpoint);
    expect(outcome.effects).toEqual([]);
  });

  it('places an out-of-order seq at its sorted position', () => {
    const outcome = applyLiveEvent(stateWith([10, 8, 6]), captureEvent(9));

    expect(seqsOf(outcome.pages)).toEqual([10, 9, 8, 6]);
    expect(outcome.endpoint?.last_seq).toBe(10);
    expect(outcome.effects).toHaveLength(1);
  });

  it('counts but does not insert a capture older than everything loaded while older pages exist', () => {
    const state = stateWith([10, 9], { pages: listOf([10, 9], { total: 20, nextCursor: 'older' }) });
    const outcome = applyLiveEvent(state, captureEvent(3));

    expect(seqsOf(outcome.pages)).toEqual([10, 9]);
    expect(outcome.pages?.pages[0]?.total).toBe(21);
    expect(outcome.effects).toEqual([]);
  });

  it('applies events in any order to the same final list when merged by id', () => {
    const order = [4, 2, 5, 3, 1, 4, 5];
    const final = order.reduce<LiveCacheState>(
      (state, seq) => ({ ...state, ...applyLiveEvent(state, captureEvent(seq)) }),
      stateWith([]),
    );

    expect(seqsOf(final.pages)).toEqual([5, 4, 3, 2, 1]);
    expect(final.pages?.pages[0]?.total).toBe(5);
  });

  it('drops the oldest rows instead of exceeding the capacity in ring mode', () => {
    const state = stateWith([5, 4, 3, 2, 1], {
      endpoint: endpointWith({ overflow: 'ring', max_captures: 5, capture_count: 5, last_seq: 5 }),
    });
    const outcome = applyLiveEvent(state, captureEvent(6));

    expect(seqsOf(outcome.pages)).toEqual([6, 5, 4, 3, 2]);
    expect(outcome.pages?.pages[0]?.total).toBe(5);
    expect(outcome.endpoint?.capture_count).toBe(5);
  });

  it('only caps the total, not the loaded rows, when older pages exist beyond capacity', () => {
    const state = stateWith([5, 4], {
      pages: listOf([5, 4], { total: 5, nextCursor: 'older' }),
      endpoint: endpointWith({ overflow: 'ring', max_captures: 5, capture_count: 5, last_seq: 5 }),
    });
    const outcome = applyLiveEvent(state, captureEvent(6));

    expect(seqsOf(outcome.pages)).toEqual([6, 5, 4]);
    expect(outcome.pages?.pages[0]?.total).toBe(5);
  });

  it('removes a deleted capture and lowers the counters', () => {
    const outcome = applyLiveEvent(stateWith([3, 2, 1]), {
      name: 'capture_deleted',
      data: { capture_id: summaryFor(2).id, seq: 2 },
    });

    expect(seqsOf(outcome.pages)).toEqual([3, 1]);
    expect(outcome.pages?.pages[0]?.total).toBe(2);
    expect(outcome.endpoint?.capture_count).toBe(2);
    expect(outcome.effects).toEqual([{ kind: 'removed', captureId: summaryFor(2).id }]);
  });

  it('does not count a delete twice when the capture is already gone locally', () => {
    const state = stateWith([3, 1], { endpoint: endpointWith({ capture_count: 2, last_seq: 3 }) });
    const outcome = applyLiveEvent(state, {
      name: 'capture_deleted',
      data: { capture_id: summaryFor(2).id, seq: 2 },
    });

    expect(outcome.pages).toBe(state.pages);
    expect(outcome.endpoint).toBe(state.endpoint);
    expect(outcome.effects).toEqual([]);
  });

  it('counts a delete that falls into the pages that are not loaded yet', () => {
    const state = stateWith([10, 9], {
      pages: listOf([10, 9], { total: 20, nextCursor: 'older' }),
      endpoint: endpointWith({ capture_count: 20, last_seq: 10 }),
    });
    const outcome = applyLiveEvent(state, {
      name: 'capture_deleted',
      data: { capture_id: summaryFor(2).id, seq: 2 },
    });

    expect(outcome.pages?.pages[0]?.total).toBe(19);
    expect(outcome.endpoint?.capture_count).toBe(19);
  });

  it('empties the list and the counters on captures_cleared', () => {
    const outcome = applyLiveEvent(stateWith([3, 2, 1]), { name: 'captures_cleared', data: {} });

    expect(seqsOf(outcome.pages)).toEqual([]);
    expect(outcome.pages?.pages[0]?.total).toBe(0);
    expect(outcome.endpoint?.capture_count).toBe(0);
    expect(outcome.endpoint?.bytes_used).toBe(0);
    expect(outcome.effects).toEqual([{ kind: 'cleared' }]);
  });

  it('replaces the endpoint settings on endpoint_updated', () => {
    const updated = endpointWith({ title: 'renamed', cors: false, capture_count: 7, last_seq: 9 });
    const outcome = applyLiveEvent(stateWith([3, 2, 1]), { name: 'endpoint_updated', data: updated });

    expect(outcome.endpoint).toBe(updated);
    expect(seqsOf(outcome.pages)).toEqual([3, 2, 1]);
  });

  it('updates the forward outcome of the capture in the list', () => {
    const state = stateWith([3, 2, 1]);
    const failed = applyLiveEvent(state, {
      name: 'forward_result',
      data: { capture_id: summaryFor(2).id, job: jobFor('failed') },
    });

    expect(failed.pages?.pages[0]?.items.map((item) => item.forward_state)).toEqual([null, 'failed', null]);

    const done = applyLiveEvent(
      { ...state, pages: failed.pages },
      {
        name: 'forward_result',
        data: { capture_id: summaryFor(2).id, job: jobFor('succeeded', 'job-2') },
      },
    );
    expect(done.pages?.pages[0]?.items[1]?.forward_state).toBe('failed');
  });

  it('lets a pending forward finish as succeeded', () => {
    const state = stateWith([2, 1]);
    const pending = applyLiveEvent(state, {
      name: 'forward_result',
      data: { capture_id: summaryFor(1).id, job: jobFor('running') },
    });
    const done = applyLiveEvent(
      { ...state, pages: pending.pages },
      {
        name: 'forward_result',
        data: { capture_id: summaryFor(1).id, job: jobFor('succeeded') },
      },
    );

    expect(pending.pages?.pages[0]?.items[1]?.forward_state).toBe('pending');
    expect(done.pages?.pages[0]?.items[1]?.forward_state).toBe('succeeded');
  });

  it('recomputes the worst forward state from the cached detail', () => {
    const base: Capture = { ...(captureSeries(2)[1] as Capture), forwards: [jobFor('failed', 'job-a')] };
    const outcome = applyLiveEvent(stateWith([2, 1], { detail: base }), {
      name: 'forward_result',
      data: { capture_id: base.id, job: jobFor('succeeded', 'job-b') },
    });

    expect(outcome.detail?.forwards.map((job) => job.id)).toEqual(['job-b', 'job-a']);
    expect(outcome.detail?.forward_state).toBe('failed');
    expect(worstForwardState(['succeeded', 'pending'])).toBe('pending');
    expect(worstForwardState([])).toBeNull();
  });

  it('asks for a catch-up on ready and on resync', () => {
    const ready = applyLiveEvent(stateWith([1]), {
      name: 'ready',
      data: { endpoint_id: ENDPOINT_ID, last_seq: 4, capture_count: 4 },
    });
    const resync = applyLiveEvent(stateWith([1]), { name: 'resync', data: { reason: 'gap' } });

    expect(ready.effects).toEqual([
      { kind: 'catchUp', reason: 'ready', ready: { endpoint_id: ENDPOINT_ID, last_seq: 4, capture_count: 4 } },
    ]);
    expect(resync.effects).toEqual([{ kind: 'catchUp', reason: 'resync', ready: null }]);
  });

  it('leaves the cache alone on bye and endpoint_deleted', () => {
    const state = stateWith([2, 1]);

    expect(applyLiveEvent(state, { name: 'bye', data: {} }).pages).toBe(state.pages);
    expect(applyLiveEvent(state, { name: 'endpoint_deleted', data: {} }).endpoint).toBe(state.endpoint);
  });

  it('works without a loaded list and counts only by the endpoint seq', () => {
    const state: LiveCacheState = {
      pages: undefined,
      order: 'desc',
      endpoint: endpointWith({ capture_count: 2, last_seq: 2 }),
      detail: undefined,
    };
    const fresh = applyLiveEvent(state, captureEvent(3));
    const stale = applyLiveEvent(state, captureEvent(2));

    expect(fresh.endpoint?.capture_count).toBe(3);
    expect(fresh.effects).toHaveLength(1);
    expect(stale.effects).toEqual([]);
  });

  it('matches the method list and the status override class on the client', () => {
    const capture = summaryFor(1, { method: 'post', status_override: 404 });

    expect(matchesActiveFilter({}, capture)).toBe(true);
    expect(matchesActiveFilter({ method: 'GET,POST' }, capture)).toBe(true);
    expect(matchesActiveFilter({ method: 'PUT' }, capture)).toBe(false);
    expect(matchesActiveFilter({ status: '4xx' }, capture)).toBe(true);
    expect(matchesActiveFilter({ status: 'none' }, capture)).toBe(false);
  });

  it('matches the received time against the inclusive start and the exclusive end', () => {
    const capture = summaryFor(2);
    const receivedAt = Date.parse(capture.received_at);
    const at = (offsetMs: number) => new Date(receivedAt + offsetMs).toISOString();

    expect(matchesActiveFilter({ since: at(0) }, capture)).toBe(true);
    expect(matchesActiveFilter({ since: at(1) }, capture)).toBe(false);
    expect(matchesActiveFilter({ until: at(0) }, capture)).toBe(false);
    expect(matchesActiveFilter({ until: at(1) }, capture)).toBe(true);
  });

  it('counts a non-matching arrival as hidden and leaves the shown pages unchanged', () => {
    const outcome = applyLiveEvent(stateWith([2, 1], { filters: { method: 'GET' } }), captureEvent(3));

    expect(seqsOf(outcome.pages)).toEqual([2, 1]);
    expect(outcome.endpoint?.capture_count).toBe(3);
    expect(outcome.effects).toEqual([{ kind: 'hidden', capture: summaryFor(3) }]);
  });

  it('asks for a server check instead of inserting an arrival while a text query is active', () => {
    const outcome = applyLiveEvent(stateWith([2, 1], { filters: { q: 'hook' } }), captureEvent(3));

    expect(seqsOf(outcome.pages)).toEqual([2, 1]);
    expect(outcome.effects).toEqual([{ kind: 'verify', capture: summaryFor(3) }]);
  });
});

describe('applyLiveEventToCache with filtered lists', () => {
  it('filters each list by its own key and reports the effects of the list on screen', () => {
    const queryClient = createSeededClient([2, 1]);
    queryClient.setQueryData(
      queryKeys.captureList(ENDPOINT_ID, { order: 'desc', limit: 100, method: 'GET' }),
      listOf([1]),
    );

    const effects = applyLiveEventToCache(queryClient, ENDPOINT_ID, captureEvent(3), { method: 'GET' });

    expect(effects).toEqual([{ kind: 'hidden', capture: summaryFor(3) }]);
    expect(seqsOf(cachedList(queryClient))).toEqual([3, 2, 1]);
    expect(
      seqsOf(
        queryClient.getQueryData(queryKeys.captureList(ENDPOINT_ID, { order: 'desc', limit: 100, method: 'GET' })),
      ),
    ).toEqual([1]);
    expect(cachedEndpoint(queryClient)?.capture_count).toBe(3);
  });
});

describe('applyLiveEventToCache', () => {
  it('writes the merged pages and the endpoint into the query cache', () => {
    const queryClient = createSeededClient([2, 1]);

    const effects = applyLiveEventToCache(queryClient, ENDPOINT_ID, captureEvent(3));

    expect(seqsOf(cachedList(queryClient))).toEqual([3, 2, 1]);
    expect(cachedEndpoint(queryClient)?.capture_count).toBe(3);
    expect(effects).toHaveLength(1);
  });

  it('merges a burst with duplicates into a clean list', () => {
    const queryClient = createSeededClient([2, 1]);

    for (const seq of [3, 4, 3, 5, 4, 2]) {
      applyLiveEventToCache(queryClient, ENDPOINT_ID, captureEvent(seq));
    }

    expect(seqsOf(cachedList(queryClient))).toEqual([5, 4, 3, 2, 1]);
    expect(cachedEndpoint(queryClient)?.capture_count).toBe(5);
  });

  it('ignores events that name another endpoint', () => {
    const queryClient = createSeededClient([2, 1]);

    const effects = applyLiveEventToCache(queryClient, ENDPOINT_ID, {
      name: 'capture',
      data: summaryFor(3, { endpoint_id: 'someone-else' }),
    });

    expect(effects).toEqual([]);
    expect(seqsOf(cachedList(queryClient))).toEqual([2, 1]);
  });

  it('updates the endpoint without a list in the cache', () => {
    const queryClient = createSeededClient([]);
    queryClient.removeQueries({ queryKey: queryKeys.captureLists(ENDPOINT_ID) });

    const effects = applyLiveEventToCache(queryClient, ENDPOINT_ID, captureEvent(1));

    expect(cachedEndpoint(queryClient)?.capture_count).toBe(1);
    expect(effects).toHaveLength(1);
  });
});

describe('live payload parsing', () => {
  it('recognises the documented event names only', () => {
    expect(isLiveEventName('capture')).toBe(true);
    expect(isLiveEventName('bye')).toBe(true);
    expect(isLiveEventName('ping')).toBe(false);
    expect(isLiveEventName('something_new')).toBe(false);
  });

  it('rejects payloads of the wrong shape', () => {
    expect(readLiveEvent('capture', { ...summaryFor(1), seq: 'one' })).toBeNull();
    expect(readLiveEvent('capture_deleted', { capture_id: 4 })).toBeNull();
    expect(readLiveEvent('resync', { reason: 'bored' })).toBeNull();
    expect(readLiveEvent('ready', { endpoint_id: 'a', last_seq: 1 })).toBeNull();
    expect(readLiveEvent('endpoint_updated', { id: 'a' })).toBeNull();
    expect(readLiveEvent('forward_result', { capture_id: 'a', job: {} })).toBeNull();
    expect(parseLiveEvent('capture', 'not json')).toBeNull();
  });

  it('accepts a summary without forward_state as having none', () => {
    const payload = { ...summaryFor(1) };
    Reflect.deleteProperty(payload, 'forward_state');
    const event = readLiveEvent('capture', payload);

    expect(event?.name === 'capture' && event.data.forward_state).toBeNull();
  });

  it('accepts the empty-payload events without reading the body', () => {
    expect(parseLiveEvent('bye', '')).toEqual({ name: 'bye', data: {} });
    expect(parseLiveEvent('endpoint_deleted', 'x')).toEqual({ name: 'endpoint_deleted', data: {} });
  });
});
