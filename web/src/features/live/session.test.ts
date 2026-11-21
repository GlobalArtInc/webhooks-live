import type { QueryClient } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { queryKeys } from '../../api/queries';
import type { CapturePage } from '../../api/types';
import type { PageLoader } from './catchUp';
import { suggestsMissedDeletes } from './catchUp';
import { createFakeEventSources, type FakeEventSources } from './fakeEventSource';
import { LiveSource } from './LiveSource';
import { cachedEndpoint, cachedList, createSeededClient, ENDPOINT_ID, seqsOf, summaryFor } from './liveFixtures';
import { LiveSession, type SessionContext } from './session';
import { LiveStore } from './store';

interface Rig {
  queryClient: QueryClient;
  fakes: FakeEventSources;
  store: LiveStore;
  session: LiveSession;
  loadPage: ReturnType<typeof vi.fn<PageLoader>>;
  followed: string[];
  context: SessionContext;
}

function pageOf(seqs: readonly number[], nextCursor: string | null = null): CapturePage {
  return { items: seqs.map((seq) => summaryFor(seq)), next_cursor: nextCursor, total: seqs.length };
}

function rig(
  seqs: readonly number[],
  pages: readonly CapturePage[] = [],
  endpoint: Parameters<typeof createSeededClient>[1] = {},
): Rig {
  const queryClient = createSeededClient(seqs, endpoint);
  const fakes = createFakeEventSources();
  const loadPage = vi.fn<PageLoader>();
  for (const page of pages) {
    loadPage.mockResolvedValueOnce(page);
  }
  loadPage.mockResolvedValue(pageOf([]));
  const source = new LiveSource({ endpointId: ENDPOINT_ID, createEventSource: fakes.factory });
  const store = new LiveStore();
  const followed: string[] = [];
  const context: SessionContext = { hidden: false, following: false };
  const session = new LiveSession({
    queryClient,
    endpointId: ENDPOINT_ID,
    source,
    store,
    loadPage,
    getContext: () => context,
    onFollow: (captureId) => followed.push(captureId),
  });
  session.start();
  fakes.latest().open();
  return { queryClient, fakes, store, session, loadPage, followed, context };
}

const EVENTUALLY_TIMEOUT_MS = 5000;

function eventually(check: () => void): Promise<void> {
  return vi.waitFor(check, { timeout: EVENTUALLY_TIMEOUT_MS });
}

function ready(lastSeq: number, captureCount: number) {
  return { endpoint_id: ENDPOINT_ID, last_seq: lastSeq, capture_count: captureCount };
}

describe('LiveSession catch-up', () => {
  let current: Rig | null = null;

  beforeEach(() => {
    current = null;
  });

  afterEach(() => {
    current?.session.dispose();
    vi.restoreAllMocks();
  });

  it('lists after the highest held seq in ascending order and merges what it finds', async () => {
    const r = rig([3, 2, 1], [pageOf([4, 5])]);
    current = r;
    r.fakes.latest().send('ready', ready(5, 5));

    await eventually(() => expect(seqsOf(cachedList(r.queryClient))).toEqual([5, 4, 3, 2, 1]));
    expect(r.loadPage).toHaveBeenCalledWith(ENDPOINT_ID, { order: 'asc', limit: 200, after_seq: 3, cursor: null });
    expect(cachedEndpoint(r.queryClient)?.capture_count).toBe(5);
  });

  it('merges catch-up pages with stream duplicates without gaps or repeats', async () => {
    let release: (page: CapturePage) => void = () => undefined;
    const r = rig([3, 2, 1]);
    current = r;
    r.loadPage.mockReset();
    r.loadPage.mockImplementationOnce(
      () =>
        new Promise<CapturePage>((resolve) => {
          release = resolve;
        }),
    );
    r.loadPage.mockResolvedValue(pageOf([]));
    const invalidate = vi.spyOn(r.queryClient, 'invalidateQueries');
    r.fakes.latest().send('ready', ready(6, 6));
    r.fakes.latest().send('capture', summaryFor(6), '6');
    r.fakes.latest().send('capture', summaryFor(4), '4');
    release(pageOf([4, 5, 6]));

    await eventually(() => expect(seqsOf(cachedList(r.queryClient))).toEqual([6, 5, 4, 3, 2, 1]));
    expect(cachedList(r.queryClient)?.pages[0]?.total).toBe(6);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.endpoint(ENDPOINT_ID) });
  });

  it('follows the cursor until the server has no more pages', async () => {
    const r = rig([2, 1], [pageOf([3, 4], 'next'), pageOf([5])]);
    current = r;
    r.fakes.latest().send('ready', ready(5, 5));

    await eventually(() => expect(seqsOf(cachedList(r.queryClient))).toEqual([5, 4, 3, 2, 1]));
    expect(r.loadPage.mock.calls.map((call) => call[1].cursor)).toEqual([null, 'next']);
    expect(r.loadPage.mock.calls.map((call) => call[1].after_seq)).toEqual([2, 2]);
  });

  it('catches up again on resync using the new highest seq', async () => {
    const r = rig([2, 1], [pageOf([]), pageOf([3])]);
    current = r;
    r.fakes.latest().send('ready', ready(2, 2));
    await eventually(() => expect(r.loadPage).toHaveBeenCalledTimes(1));
    r.fakes.latest().send('resync', { reason: 'lagged' });

    await eventually(() => expect(seqsOf(cachedList(r.queryClient))).toEqual([3, 2, 1]));
    expect(r.loadPage.mock.calls[1]?.[1].after_seq).toBe(2);
  });

  it('refills a hole the stream left below the highest seq when it resyncs', async () => {
    const r = rig([5, 4, 3, 2, 1], [pageOf([]), pageOf([6, 7])]);
    current = r;
    r.fakes.latest().send('ready', ready(5, 5));
    await eventually(() => expect(r.loadPage).toHaveBeenCalledTimes(1));
    r.fakes.latest().send('capture', summaryFor(7), '7');
    expect(seqsOf(cachedList(r.queryClient))).toEqual([7, 5, 4, 3, 2, 1]);

    r.fakes.latest().send('resync', { reason: 'lagged' });

    await eventually(() => expect(seqsOf(cachedList(r.queryClient))).toEqual([7, 6, 5, 4, 3, 2, 1]));
    expect(r.loadPage.mock.calls[1]?.[1].after_seq).toBe(5);
  });

  it('catches up when a payload could not be understood', async () => {
    const r = rig([2, 1], [pageOf([3])]);
    current = r;
    r.fakes.latest().sendRaw('capture', '{broken');

    await eventually(() => expect(seqsOf(cachedList(r.queryClient))).toEqual([3, 2, 1]));
  });

  it('refetches the list when the ready count suggests deletes it missed', async () => {
    const r = rig([3, 2, 1]);
    current = r;
    const invalidate = vi.spyOn(r.queryClient, 'invalidateQueries');
    r.fakes.latest().send('ready', ready(3, 2));

    await eventually(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.captureLists(ENDPOINT_ID) }));
  });

  it('keeps the list when the ready count matches what the cache holds', async () => {
    const r = rig([3, 2, 1], [pageOf([4])]);
    current = r;
    const invalidate = vi.spyOn(r.queryClient, 'invalidateQueries');
    r.fakes.latest().send('ready', ready(4, 4));

    await eventually(() => expect(seqsOf(cachedList(r.queryClient))).toEqual([4, 3, 2, 1]));
    await Promise.resolve();
    expect(invalidate).not.toHaveBeenCalledWith({ queryKey: queryKeys.captureLists(ENDPOINT_ID) });
  });

  it('refetches instead of patching when the catch-up request fails', async () => {
    const r = rig([2, 1]);
    current = r;
    r.loadPage.mockReset();
    r.loadPage.mockRejectedValue(new TypeError('network'));
    const invalidate = vi.spyOn(r.queryClient, 'invalidateQueries');
    r.fakes.latest().send('ready', ready(2, 2));

    await eventually(() => expect(invalidate).toHaveBeenCalledWith({ queryKey: queryKeys.captureLists(ENDPOINT_ID) }));
  });

  it('judges missed deletes by the ready count plus what arrived since, bounded by capacity', () => {
    expect(suggestsMissedDeletes({ readyCount: 5, deltaSinceReady: 2, total: 7, capacity: 500 })).toBe(false);
    expect(suggestsMissedDeletes({ readyCount: 5, deltaSinceReady: 0, total: 4, capacity: 500 })).toBe(true);
    expect(suggestsMissedDeletes({ readyCount: 5, deltaSinceReady: 3, total: 5, capacity: 5 })).toBe(false);
    expect(suggestsMissedDeletes({ readyCount: 5, deltaSinceReady: 0, total: null, capacity: 5 })).toBe(false);
  });
});

describe('LiveSession arrivals', () => {
  let current: Rig | null = null;

  afterEach(() => {
    current?.session.dispose();
    current?.store.dispose();
    current = null;
  });

  it('marks arrivals unread when they do not become the selection', () => {
    const r = rig([2, 1]);
    current = r;
    r.fakes.latest().send('capture', summaryFor(3), '3');
    r.fakes.latest().send('capture', summaryFor(4), '4');

    expect([...r.store.getSnapshot().unreadIds]).toEqual([summaryFor(3).id, summaryFor(4).id]);
    expect(r.store.getSnapshot().awayCount).toBe(0);
    expect(r.store.getSnapshot().arrivingIds.size).toBe(2);
    expect(r.followed).toEqual([]);
  });

  it('counts arrivals while the tab is hidden for the title and skips the animation', () => {
    const r = rig([2, 1]);
    current = r;
    r.context.hidden = true;
    r.fakes.latest().send('capture', summaryFor(3), '3');

    expect(r.store.getSnapshot().awayCount).toBe(1);
    expect(r.store.getSnapshot().arrivingIds.size).toBe(0);
    expect(r.store.getSnapshot().announcement).toBeNull();
  });

  it('follows the newest arrival and does not mark it unread while following', () => {
    const r = rig([2, 1]);
    current = r;
    r.context.following = true;
    r.fakes.latest().send('capture', summaryFor(3), '3');

    expect(r.followed).toEqual([summaryFor(3).id]);
    expect(r.store.getSnapshot().unreadIds.size).toBe(0);
  });

  it('forgets an unread capture when it is deleted elsewhere and resets on clear', () => {
    const r = rig([2, 1]);
    current = r;
    r.fakes.latest().send('capture', summaryFor(3), '3');
    r.fakes.latest().send('capture_deleted', { capture_id: summaryFor(3).id, seq: 3 });
    expect(r.store.getSnapshot().unreadIds.size).toBe(0);

    r.fakes.latest().send('capture', summaryFor(4), '4');
    r.fakes.latest().send('captures_cleared', {});
    expect(r.store.getSnapshot().unreadIds.size).toBe(0);
    expect(seqsOf(cachedList(r.queryClient))).toEqual([]);
  });

  it('mirrors connection changes and the last event time into the store', () => {
    const r = rig([1]);
    current = r;
    expect(r.store.getSnapshot().connection).toBe('open');

    r.fakes.latest().send('endpoint_deleted', {});

    expect(r.store.getSnapshot().connection).toBe('ended');
    expect(r.store.getSnapshot().endReason).toBe('deleted');
    expect(r.store.getSnapshot().lastEventAt).not.toBeNull();
  });
});
