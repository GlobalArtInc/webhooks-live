import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setGrantRejectedHandler } from '../../api/client';
import { clearGrant, readGrant, storeGrant } from '../access/grants';
import { backoffDelay } from './backoff';
import { createFakeEventSources, type FakeEventSources } from './fakeEventSource';
import { LiveSource, type StatusProbe } from './LiveSource';
import type { ConnectionState, EndReason } from './types';
import { summaryFor } from './liveFixtures';

const ID = '7k2m9q4xw8c1d5f3h6j0n2p4';
const MIDDLE = 0.5;

function setHidden(value: boolean): void {
  Object.defineProperty(document, 'hidden', { configurable: true, get: () => value });
  document.dispatchEvent(new Event('visibilitychange'));
}

function setOnline(value: boolean): void {
  Object.defineProperty(window.navigator, 'onLine', { configurable: true, get: () => value });
  window.dispatchEvent(new Event(value ? 'online' : 'offline'));
}

interface Harness {
  fakes: FakeEventSources;
  source: LiveSource;
  states: ConnectionState[];
  reasons: (EndReason | null)[];
}

function harness(probe?: StatusProbe, getLastSeq?: () => number | null): Harness {
  const fakes = createFakeEventSources();
  const source = new LiveSource({
    endpointId: ID,
    createEventSource: fakes.factory,
    random: () => MIDDLE,
    probe,
    getLastSeq,
  });
  const states: ConnectionState[] = [];
  const reasons: (EndReason | null)[] = [];
  source.on('state', (change) => {
    states.push(change.state);
    reasons.push(change.reason);
  });
  source.start();
  return { fakes, source, states, reasons };
}

describe('LiveSource', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    Reflect.deleteProperty(document, 'hidden');
    Reflect.deleteProperty(window.navigator, 'onLine');
  });

  it('connects to the stream of the endpoint and reports open', () => {
    const { fakes, source } = harness();

    expect(fakes.latest().url).toBe(`/api/v1/endpoints/${ID}/stream`);
    expect(source.getState()).toBe('connecting');
    fakes.latest().open();
    expect(source.getState()).toBe('open');
    source.close();
  });

  it('retries with exponential backoff and jitter bounds, capped at thirty seconds', () => {
    const { fakes, source } = harness();
    const delays: number[] = [];
    for (let attempt = 0; attempt < 7; attempt += 1) {
      const before = fakes.instances.length;
      fakes.latest().fail(false);
      expect(source.getState()).toBe('reconnecting');
      const delay = backoffDelay(attempt, MIDDLE);
      delays.push(delay);
      vi.advanceTimersByTime(delay - 1);
      expect(fakes.instances).toHaveLength(before);
      vi.advanceTimersByTime(1);
      expect(fakes.instances).toHaveLength(before + 1);
    }

    expect(delays).toEqual([1000, 2000, 4000, 8000, 16_000, 30_000, 30_000]);
    expect(backoffDelay(0, 0)).toBe(1000);
    expect(backoffDelay(0, 1)).toBe(1200);
    expect(backoffDelay(3, 0)).toBe(6400);
    expect(backoffDelay(20, 1)).toBe(30_000);
    source.close();
  });

  it('resets the backoff after thirty seconds of a stable connection', () => {
    const { fakes, source } = harness();
    fakes.latest().fail(false);
    vi.advanceTimersByTime(1000);
    fakes.latest().fail(false);
    vi.advanceTimersByTime(2000);
    fakes.latest().open();
    vi.advanceTimersByTime(30_000);
    const before = fakes.instances.length;
    fakes.latest().fail(false);

    vi.advanceTimersByTime(999);
    expect(fakes.instances).toHaveLength(before);
    vi.advanceTimersByTime(1);
    expect(fakes.instances).toHaveLength(before + 1);
    source.close();
  });

  it('keeps growing the backoff when the connection drops before it was stable', () => {
    const { fakes, source } = harness();
    fakes.latest().open();
    vi.advanceTimersByTime(5000);
    fakes.latest().fail(false);
    vi.advanceTimersByTime(1000);
    fakes.latest().open();
    vi.advanceTimersByTime(5000);
    const before = fakes.instances.length;
    fakes.latest().fail(false);

    vi.advanceTimersByTime(1999);
    expect(fakes.instances).toHaveLength(before);
    vi.advanceTimersByTime(1);
    expect(fakes.instances).toHaveLength(before + 1);
    source.close();
  });

  it('closes the stream after sixty hidden seconds and reconnects at once on return', () => {
    const { fakes, source, states } = harness();
    fakes.latest().open();
    setHidden(true);
    vi.advanceTimersByTime(59_999);
    expect(fakes.latest().isClosed()).toBe(false);
    vi.advanceTimersByTime(1);

    expect(fakes.latest().isClosed()).toBe(true);
    expect(source.getState()).toBe('offline');
    const count = fakes.instances.length;
    vi.advanceTimersByTime(10 * 60_000);
    expect(fakes.instances).toHaveLength(count);
    expect(vi.getTimerCount()).toBe(0);

    setHidden(false);
    expect(fakes.instances).toHaveLength(count + 1);
    expect(states.at(-1)).toBe('reconnecting');
    source.close();
  });

  it('keeps the stream when the tab returns within the grace period', () => {
    const { fakes, source } = harness();
    fakes.latest().open();
    setHidden(true);
    vi.advanceTimersByTime(30_000);
    setHidden(false);
    vi.advanceTimersByTime(120_000);

    expect(fakes.instances).toHaveLength(1);
    expect(source.getState()).toBe('open');
    source.close();
  });

  it('starts no retry timer while the tab is hidden', () => {
    const { fakes, source } = harness();
    fakes.latest().open();
    setHidden(true);
    fakes.latest().fail(false);

    expect(source.getState()).toBe('offline');
    expect(fakes.instances).toHaveLength(1);
    vi.advanceTimersByTime(5 * 60_000);
    expect(fakes.instances).toHaveLength(1);
    source.close();
  });

  it('goes offline when the browser goes offline and reconnects immediately on the online event', () => {
    const { fakes, source } = harness();
    fakes.latest().open();
    setOnline(false);
    expect(source.getState()).toBe('offline');
    expect(fakes.latest().isClosed()).toBe(true);
    vi.advanceTimersByTime(120_000);
    expect(fakes.instances).toHaveLength(1);

    setOnline(true);

    expect(fakes.instances).toHaveLength(2);
    expect(source.getState()).toBe('reconnecting');
    source.close();
  });

  it('skips a pending backoff wait on the online event', () => {
    const { fakes, source } = harness();
    fakes.latest().fail(false);
    expect(source.getState()).toBe('reconnecting');

    window.dispatchEvent(new Event('online'));

    expect(fakes.instances).toHaveLength(2);
    vi.advanceTimersByTime(60_000);
    expect(fakes.instances).toHaveLength(2);
    source.close();
  });

  it('reconnects at once after a bye event', () => {
    const { fakes, source } = harness();
    fakes.latest().open();
    vi.advanceTimersByTime(10_000);
    fakes.latest().send('bye', {});

    expect(fakes.instances).toHaveLength(2);
    expect(fakes.instances[0]?.isClosed()).toBe(true);
    source.close();
  });

  it('backs off when a connection opened during drain says bye straight away', () => {
    const { fakes, source } = harness();
    fakes.latest().open();
    fakes.latest().send('bye', {});

    expect(fakes.instances).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    expect(fakes.instances).toHaveLength(2);
    source.close();
  });

  it('passes the highest seen seq to every manual reconnect as last_event_id', () => {
    let held: number | null = null;
    const { fakes, source } = harness(undefined, () => held);
    expect(fakes.latest().url).not.toContain('last_event_id');
    fakes.latest().open();
    fakes.latest().send('capture', summaryFor(7), '7');
    fakes.latest().send('capture', summaryFor(9), '9');
    fakes.latest().send('capture', summaryFor(8), '8');
    fakes.latest().fail(false);
    vi.advanceTimersByTime(1000);

    expect(fakes.latest().url).toBe(`/api/v1/endpoints/${ID}/stream?last_event_id=9`);

    held = 12;
    fakes.latest().fail(false);
    vi.advanceTimersByTime(2000);
    expect(fakes.latest().url).toBe(`/api/v1/endpoints/${ID}/stream?last_event_id=12`);
    source.close();
  });

  it('opens the first connection with the seq the cache already holds', () => {
    const { fakes, source } = harness(undefined, () => 41);

    expect(fakes.latest().url).toBe(`/api/v1/endpoints/${ID}/stream?last_event_id=41`);
    source.close();
  });

  it('reports a gone endpoint as an ended state and stops retrying', async () => {
    const probe = vi.fn<StatusProbe>().mockResolvedValue(410);
    const { fakes, source, states, reasons } = harness(probe);
    fakes.latest().fail(true);
    await vi.advanceTimersByTimeAsync(0);

    expect(probe).toHaveBeenCalledWith(ID);
    expect(source.getState()).toBe('ended');
    expect(states.at(-1)).toBe('ended');
    expect(reasons.at(-1)).toBe('gone');
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(fakes.instances).toHaveLength(1);
    source.close();
  });

  it('reports a missing endpoint as ended with its own reason', async () => {
    const { fakes, source } = harness(vi.fn<StatusProbe>().mockResolvedValue(404));
    fakes.latest().fail(true);
    await vi.advanceTimersByTimeAsync(0);

    expect(source.getEndReason()).toBe('missing');
    source.close();
  });

  it('ends as locked when the probe answers 401, drops the grant, tells the handler, and stops retrying', async () => {
    const rejected = vi.fn<(endpointId: string) => void>();
    setGrantRejectedHandler(rejected);
    storeGrant(ID, 'fake-grant', '2999-01-01T00:00:00.000Z');
    const { fakes, source } = harness(vi.fn<StatusProbe>().mockResolvedValue(401));
    fakes.latest().fail(true);
    await vi.advanceTimersByTimeAsync(0);

    expect(source.getState()).toBe('ended');
    expect(source.getEndReason()).toBe('locked');
    expect(readGrant(ID)).toBeNull();
    expect(rejected).toHaveBeenCalledWith(ID);
    await vi.advanceTimersByTimeAsync(5 * 60_000);
    expect(fakes.instances).toHaveLength(1);
    source.close();
    setGrantRejectedHandler(null);
    clearGrant(ID);
  });

  it('adds the grant to the stream address', () => {
    storeGrant(ID, 'fake-grant', '2999-01-01T00:00:00.000Z');
    const { fakes, source } = harness(undefined, () => 7);

    expect(fakes.latest().url).toBe(`/api/v1/endpoints/${ID}/stream?last_event_id=7&grant=fake-grant`);
    source.close();
    clearGrant(ID);
  });

  it('re-opens the open stream with the new grant when one is adopted, and closes the old source', () => {
    const { fakes, source } = harness();
    const first = fakes.latest();
    first.open();
    expect(first.url).not.toContain('grant=');

    storeGrant(ID, 'adopted-grant', '2999-01-01T00:00:00.000Z');

    expect(first.isClosed()).toBe(true);
    expect(fakes.instances).toHaveLength(2);
    expect(fakes.latest().url).toBe(`/api/v1/endpoints/${ID}/stream?grant=adopted-grant`);
    source.close();
    clearGrant(ID);
  });

  it('leaves a locked stream and reconnects with a grant that arrives afterwards', async () => {
    setGrantRejectedHandler(vi.fn<(endpointId: string) => void>());
    const { fakes, source } = harness(vi.fn<StatusProbe>().mockResolvedValue(401));
    fakes.latest().fail(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(source.getEndReason()).toBe('locked');

    storeGrant(ID, 'late-grant', '2999-01-01T00:00:00.000Z');

    expect(fakes.instances).toHaveLength(2);
    expect(fakes.latest().url).toBe(`/api/v1/endpoints/${ID}/stream?grant=late-grant`);
    expect(source.getEndReason()).toBeNull();
    fakes.latest().open();
    expect(source.getState()).toBe('open');
    source.close();
    setGrantRejectedHandler(null);
    clearGrant(ID);
  });

  it('ignores grants of other endpoints and grants stored after it was closed', () => {
    const { fakes, source } = harness();
    storeGrant('other00000000000000000000', 'x', '2999-01-01T00:00:00.000Z');
    expect(fakes.instances).toHaveLength(1);
    source.close();
    storeGrant(ID, 'after-close', '2999-01-01T00:00:00.000Z');
    expect(fakes.instances).toHaveLength(1);
    clearGrant(ID);
    clearGrant('other00000000000000000000');
  });

  it('retries after a closed stream when the endpoint itself answers normally', async () => {
    const { fakes, source } = harness(vi.fn<StatusProbe>().mockResolvedValue(503));
    fakes.latest().fail(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(source.getState()).toBe('reconnecting');
    await vi.advanceTimersByTimeAsync(1000);

    expect(fakes.instances).toHaveLength(2);
    source.close();
  });

  it('retries after a closed stream when the probe itself fails', async () => {
    const { fakes, source } = harness(vi.fn<StatusProbe>().mockRejectedValue(new TypeError('network')));
    fakes.latest().fail(true);
    await vi.advanceTimersByTimeAsync(1000);

    expect(fakes.instances).toHaveLength(2);
    source.close();
  });

  it('stops for good on endpoint_deleted after delivering the event', () => {
    const { fakes, source, reasons } = harness();
    const received: string[] = [];
    source.on('endpoint_deleted', () => received.push('deleted'));
    fakes.latest().open();
    fakes.latest().send('endpoint_deleted', {});

    expect(received).toEqual(['deleted']);
    expect(source.getState()).toBe('ended');
    expect(reasons.at(-1)).toBe('deleted');
    expect(fakes.latest().isClosed()).toBe(true);
    vi.advanceTimersByTime(60_000);
    expect(fakes.instances).toHaveLength(1);
    source.close();
  });

  it('delivers typed events and reports malformed payloads instead of dropping them silently', () => {
    const { fakes, source } = harness();
    const captures: number[] = [];
    const invalid: string[] = [];
    source.on('capture', (summary) => captures.push(summary.seq));
    source.on('invalid', (event) => invalid.push(event.name));
    fakes.latest().open();
    fakes.latest().send('capture', summaryFor(3), '3');
    fakes.latest().sendRaw('capture', '{not json');
    fakes.latest().send('capture', { id: 'x' });

    expect(captures).toEqual([3]);
    expect(invalid).toEqual(['capture', 'capture']);
    source.close();
  });

  it('ignores events that arrive from a stream it already replaced', () => {
    const { fakes, source } = harness();
    const captures: number[] = [];
    source.on('capture', (summary) => captures.push(summary.seq));
    const stale = fakes.latest();
    stale.fail(false);
    vi.advanceTimersByTime(1000);
    stale.send('capture', summaryFor(5), '5');

    expect(captures).toEqual([]);
    source.close();
  });

  it('stops everything when closed', () => {
    const { fakes, source } = harness();
    fakes.latest().open();
    source.close();

    expect(fakes.latest().isClosed()).toBe(true);
    setHidden(true);
    setOnline(false);
    setOnline(true);
    vi.advanceTimersByTime(10 * 60_000);
    expect(fakes.instances).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('LiveSource with a multiplexed stream', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('resumes from the last event id it received, as the opaque vector id', () => {
    const fakes = createFakeEventSources();
    const source = new LiveSource({
      buildUrl: (lastEventId) =>
        lastEventId === null ? '/api/v1/inbox/stream' : `/api/v1/inbox/stream?last_event_id=${lastEventId}`,
      createEventSource: fakes.factory,
      random: () => MIDDLE,
    });
    source.start();
    fakes.latest().open();
    fakes.latest().send('capture', summaryFor(1), 'v-7');
    fakes.latest().fail(false);
    vi.advanceTimersByTime(backoffDelay(0, MIDDLE));
    expect(fakes.latest().url).toBe('/api/v1/inbox/stream?last_event_id=v-7');
    source.close();
  });

  it('keeps the stream open when one endpoint of the set is deleted', () => {
    const fakes = createFakeEventSources();
    const source = new LiveSource({
      buildUrl: () => '/api/v1/inbox/stream',
      createEventSource: fakes.factory,
      finishOnEndpointDeleted: false,
    });
    const deleted: string[] = [];
    source.on('endpoint_deleted', () => deleted.push('deleted'));
    source.start();
    fakes.latest().open();
    fakes.latest().send('endpoint_deleted', {});
    expect(deleted).toEqual(['deleted']);
    expect(source.getState()).toBe('open');
    expect(source.getEndReason()).toBeNull();
    source.close();
  });
});
