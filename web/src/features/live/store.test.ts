import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ANNOUNCE_INTERVAL_MS, announcedPath, EMPTY_ANNOUNCER, addArrivals, stepAnnouncer } from './announcer';
import { summaryFor } from './liveFixtures';
import { ARRIVAL_SETTLE_MS, LiveStore, MAX_UNREAD } from './store';

const VISIBLE = { hidden: false, following: false };

describe('announcer plan', () => {
  it('speaks the first arrival at once with its method and path', () => {
    const step = stepAnnouncer(addArrivals(EMPTY_ANNOUNCER, [summaryFor(1, { path: '/stripe' })]), 1000);

    expect(step.announcement).toEqual({ id: 1, count: 1, method: 'POST', path: '/stripe' });
    expect(step.state.pending).toBe(0);
  });

  it('holds later arrivals back until five seconds passed and then aggregates them', () => {
    const spoken = stepAnnouncer(addArrivals(EMPTY_ANNOUNCER, [summaryFor(1)]), 1000);
    const queued = addArrivals(spoken.state, [summaryFor(2), summaryFor(3, { path: '/stripe' })]);
    const early = stepAnnouncer(queued, 1000 + ANNOUNCE_INTERVAL_MS - 1);
    const due = stepAnnouncer(queued, 1000 + ANNOUNCE_INTERVAL_MS);

    expect(early.announcement).toBeNull();
    expect(early.waitMs).toBe(1);
    expect(due.announcement).toEqual({ id: 2, count: 2, method: 'POST', path: '/stripe' });
  });

  it('keeps the newest capture as the latest one even when they arrive out of order', () => {
    const state = addArrivals(EMPTY_ANNOUNCER, [summaryFor(5, { path: '/five' }), summaryFor(4, { path: '/four' })]);

    expect(stepAnnouncer(state, 0).announcement?.path).toBe('/five');
  });

  it('says nothing when nothing is pending and shortens very long paths', () => {
    expect(stepAnnouncer(EMPTY_ANNOUNCER, 0)).toEqual({ state: EMPTY_ANNOUNCER, announcement: null, waitMs: null });
    expect(announcedPath('')).toBe('/');
    expect(announcedPath(`/${'a'.repeat(100)}`)).toHaveLength(61);
  });
});

describe('LiveStore', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps unread captures until they are marked read', () => {
    const store = new LiveStore();
    store.recordArrivals([summaryFor(1), summaryFor(2)], VISIBLE);
    store.markRead(summaryFor(1).id);

    expect([...store.getSnapshot().unreadIds]).toEqual([summaryFor(2).id]);
    store.dispose();
  });

  it('does not mark arrivals unread while following', () => {
    const store = new LiveStore();
    store.recordArrivals([summaryFor(1)], { hidden: false, following: true });

    expect(store.getSnapshot().unreadIds.size).toBe(0);
    store.dispose();
  });

  it('counts arrivals while hidden for the badge and clears the count on return', () => {
    const store = new LiveStore();
    store.recordArrivals([summaryFor(1), summaryFor(2)], { hidden: true, following: false });
    store.recordArrivals([summaryFor(3)], { hidden: true, following: false });

    expect(store.getSnapshot().awayCount).toBe(3);
    expect(store.getSnapshot().arrivingIds.size).toBe(0);
    store.clearAway();
    expect(store.getSnapshot().awayCount).toBe(0);
    store.dispose();
  });

  it('bounds the unread set', () => {
    const store = new LiveStore();
    const many = Array.from({ length: MAX_UNREAD + 5 }, (_value, index) => summaryFor(index + 1));
    store.recordArrivals(many, VISIBLE);

    expect(store.getSnapshot().unreadIds.size).toBe(MAX_UNREAD);
    expect(store.getSnapshot().unreadIds.has(summaryFor(1).id)).toBe(false);
    expect(store.getSnapshot().unreadIds.has(summaryFor(MAX_UNREAD + 5).id)).toBe(true);
    store.dispose();
  });

  it('stops marking rows as arriving once the animation has run', () => {
    const store = new LiveStore();
    store.recordArrivals([summaryFor(1)], VISIBLE);
    expect(store.getSnapshot().arrivingIds.size).toBe(1);

    vi.advanceTimersByTime(ARRIVAL_SETTLE_MS);

    expect(store.getSnapshot().arrivingIds.size).toBe(0);
    store.dispose();
  });

  it('announces at most one aggregated message per five seconds', () => {
    const store = new LiveStore();
    const messages: number[] = [];
    store.subscribe(() => {
      const announcement = store.getSnapshot().announcement;
      if (announcement !== null && messages.at(-1) !== announcement.id) {
        messages.push(announcement.id);
      }
    });

    store.recordArrivals([summaryFor(1)], VISIBLE);
    for (const seq of [2, 3, 4]) {
      vi.advanceTimersByTime(1000);
      store.recordArrivals([summaryFor(seq, { path: '/last' })], VISIBLE);
    }
    expect(messages).toEqual([1]);

    vi.advanceTimersByTime(ANNOUNCE_INTERVAL_MS);

    expect(messages).toEqual([1, 2]);
    expect(store.getSnapshot().announcement).toEqual({ id: 2, count: 3, method: 'POST', path: '/last' });
    vi.advanceTimersByTime(60_000);
    expect(messages).toEqual([1, 2]);
    store.dispose();
  });

  it('stays silent about arrivals while the tab is hidden', () => {
    const store = new LiveStore();
    store.recordArrivals([summaryFor(1)], { hidden: true, following: false });
    vi.advanceTimersByTime(60_000);

    expect(store.getSnapshot().announcement).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
    store.dispose();
  });

  it('forgets a capture everywhere and resets on clear', () => {
    const store = new LiveStore();
    store.recordArrivals([summaryFor(1), summaryFor(2)], VISIBLE);
    store.forget(summaryFor(1).id);
    expect(store.getSnapshot().unreadIds.has(summaryFor(1).id)).toBe(false);
    expect(store.getSnapshot().arrivingIds.has(summaryFor(1).id)).toBe(false);

    store.reset();

    expect(store.getSnapshot().unreadIds.size).toBe(0);
    expect(store.getSnapshot().announcement).toBeNull();
    expect(vi.getTimerCount()).toBe(0);
  });
});
