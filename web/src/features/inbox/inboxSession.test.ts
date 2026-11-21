import { QueryClient } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { backoffDelay } from '../live/backoff';
import { createFakeEventSources } from '../live/fakeEventSource';
import { LiveSource } from '../live/LiveSource';
import { summaryFor } from '../live/liveFixtures';
import { inboxListKey, type InboxListData } from './cache';
import { InboxSession } from './inboxSession';
import { inboxStreamUrl } from './stream';

const MIDDLE = 0.5;
const FIRST = '0123456789abcdefghjkmnpq';
const SECOND = '0123456789abcdefghjkmnpr';
const SCOPE = 'all';

function idsOf(queryClient: QueryClient): string[] {
  const data = queryClient.getQueryData<InboxListData>(inboxListKey(SCOPE));
  return (data?.pages.flatMap((page) => page.items) ?? []).map((item) => item.id);
}

describe('InboxSession', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('merges arrivals from two endpoints once after a forced reconnect replays them', () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData<InboxListData>(inboxListKey(SCOPE), {
      pages: [{ items: [summaryFor(1, { endpoint_id: FIRST })], next_cursor: null }],
      pageParams: [null],
    });
    const fakes = createFakeEventSources();
    const source = new LiveSource({
      buildUrl: (lastEventId) => inboxStreamUrl({}, lastEventId),
      createEventSource: fakes.factory,
      random: () => MIDDLE,
      finishOnEndpointDeleted: false,
    });
    const session = new InboxSession({
      queryClient,
      queryKey: inboxListKey(SCOPE),
      source,
      onActivity: () => undefined,
    });
    session.start();
    fakes.latest().open();

    const arrivalFromSecond = summaryFor(3, { endpoint_id: SECOND });
    fakes.latest().send('capture', arrivalFromSecond, 'vector-1');
    expect(idsOf(queryClient)).toEqual([arrivalFromSecond.id, summaryFor(1).id]);

    fakes.latest().fail(false);
    vi.advanceTimersByTime(backoffDelay(0, MIDDLE));
    expect(fakes.latest().url).toBe('/api/v1/inbox/stream?last_event_id=vector-1');

    fakes.latest().open();
    fakes.latest().send('capture', arrivalFromSecond, 'vector-2');
    const newer = summaryFor(4, { endpoint_id: FIRST });
    fakes.latest().send('capture', newer, 'vector-3');

    const ids = idsOf(queryClient);
    expect(ids).toEqual([newer.id, arrivalFromSecond.id, summaryFor(1).id]);
    expect(new Set(ids).size).toBe(ids.length);
    session.dispose();
  });

  it('removes a deleted capture from the merged list by its id', () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData<InboxListData>(inboxListKey(SCOPE), {
      pages: [{ items: [summaryFor(2), summaryFor(1)], next_cursor: null }],
      pageParams: [null],
    });
    const fakes = createFakeEventSources();
    const source = new LiveSource({ createEventSource: fakes.factory, finishOnEndpointDeleted: false });
    const session = new InboxSession({
      queryClient,
      queryKey: inboxListKey(SCOPE),
      source,
      onActivity: () => undefined,
    });
    session.start();
    fakes.latest().open();
    fakes.latest().send('capture_deleted', { capture_id: summaryFor(2).id, seq: 2 });
    expect(idsOf(queryClient)).toEqual([summaryFor(1).id]);
    session.dispose();
  });
});
