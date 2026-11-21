import type { CaptureSummary } from '../../api/types';
import { addArrivals, EMPTY_ANNOUNCER, stepAnnouncer, type Announcement, type AnnouncerState } from './announcer';
import type { ConnectionState, EndReason } from './types';

export const MAX_UNREAD = 1000;
export const MAX_ARRIVING = 30;
export const ARRIVAL_SETTLE_MS = 1700;

type Listener = () => void;
type Timer = ReturnType<typeof setTimeout>;

export interface LiveSnapshot {
  connection: ConnectionState;
  endReason: EndReason | null;
  lastEventAt: number | null;
  unreadIds: ReadonlySet<string>;
  awayCount: number;
  hiddenCount: number;
  arrivingIds: ReadonlySet<string>;
  announcement: Announcement | null;
}

export interface ArrivalContext {
  hidden: boolean;
  following: boolean;
}

export interface StoreClock {
  now: () => number;
}

const NO_IDS: ReadonlySet<string> = new Set();

function initialSnapshot(): LiveSnapshot {
  return {
    connection: 'connecting',
    endReason: null,
    lastEventAt: null,
    unreadIds: NO_IDS,
    awayCount: 0,
    hiddenCount: 0,
    arrivingIds: NO_IDS,
    announcement: null,
  };
}

function withoutId(ids: ReadonlySet<string>, captureId: string): ReadonlySet<string> {
  if (!ids.has(captureId)) {
    return ids;
  }
  const next = new Set(ids);
  next.delete(captureId);
  return next;
}

function withIds(ids: ReadonlySet<string>, added: readonly string[], limit: number): ReadonlySet<string> {
  const next = new Set(ids);
  for (const captureId of added) {
    next.add(captureId);
  }
  const excess = next.size - limit;
  if (excess > 0) {
    for (const captureId of [...next].slice(0, excess)) {
      next.delete(captureId);
    }
  }
  return next;
}

export class LiveStore {
  private snapshot: LiveSnapshot = initialSnapshot();
  private readonly listeners = new Set<Listener>();
  private readonly clock: StoreClock;
  private announcer: AnnouncerState = EMPTY_ANNOUNCER;
  private settleTimer: Timer | null = null;
  private announceTimer: Timer | null = null;

  constructor(clock: StoreClock = { now: Date.now }) {
    this.clock = clock;
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  getSnapshot = (): LiveSnapshot => this.snapshot;

  private update(change: Partial<LiveSnapshot>): void {
    this.snapshot = { ...this.snapshot, ...change };
    for (const listener of [...this.listeners]) {
      listener();
    }
  }

  setConnection(connection: ConnectionState, endReason: EndReason | null): void {
    if (connection === this.snapshot.connection && endReason === this.snapshot.endReason) {
      return;
    }
    this.update({ connection, endReason });
  }

  touch(at: number): void {
    this.update({ lastEventAt: at });
  }

  recordArrivals(captures: readonly CaptureSummary[], context: ArrivalContext): void {
    if (captures.length === 0) {
      return;
    }
    const ids = captures.map((capture) => capture.id);
    const change: Partial<LiveSnapshot> = {};
    if (context.hidden) {
      change.awayCount = this.snapshot.awayCount + captures.length;
    } else {
      change.arrivingIds = withIds(this.snapshot.arrivingIds, ids, MAX_ARRIVING);
      this.armSettle();
      this.announce(captures);
    }
    if (!context.following) {
      change.unreadIds = withIds(this.snapshot.unreadIds, ids, MAX_UNREAD);
    }
    this.update(change);
  }

  markRead(captureId: string): void {
    const unreadIds = withoutId(this.snapshot.unreadIds, captureId);
    if (unreadIds !== this.snapshot.unreadIds) {
      this.update({ unreadIds });
    }
  }

  forget(captureId: string): void {
    const unreadIds = withoutId(this.snapshot.unreadIds, captureId);
    const arrivingIds = withoutId(this.snapshot.arrivingIds, captureId);
    if (unreadIds !== this.snapshot.unreadIds || arrivingIds !== this.snapshot.arrivingIds) {
      this.update({ unreadIds, arrivingIds });
    }
  }

  clearAway(): void {
    if (this.snapshot.awayCount !== 0) {
      this.update({ awayCount: 0 });
    }
  }

  recordHidden(count: number): void {
    if (count > 0) {
      this.update({ hiddenCount: this.snapshot.hiddenCount + count });
    }
  }

  clearHidden(): void {
    if (this.snapshot.hiddenCount !== 0) {
      this.update({ hiddenCount: 0 });
    }
  }

  reset(): void {
    this.announcer = EMPTY_ANNOUNCER;
    this.disarm();
    this.update({ unreadIds: NO_IDS, arrivingIds: NO_IDS, awayCount: 0, announcement: null });
  }

  dispose(): void {
    this.disarm();
  }

  private disarm(): void {
    if (this.settleTimer !== null) {
      clearTimeout(this.settleTimer);
      this.settleTimer = null;
    }
    if (this.announceTimer !== null) {
      clearTimeout(this.announceTimer);
      this.announceTimer = null;
    }
  }

  private armSettle(): void {
    if (this.settleTimer !== null) {
      clearTimeout(this.settleTimer);
    }
    this.settleTimer = setTimeout(() => {
      this.settleTimer = null;
      if (this.snapshot.arrivingIds.size > 0) {
        this.update({ arrivingIds: NO_IDS });
      }
    }, ARRIVAL_SETTLE_MS);
  }

  private announce(captures: readonly CaptureSummary[]): void {
    this.announcer = addArrivals(this.announcer, captures);
    this.flushAnnouncer();
  }

  private flushAnnouncer(): void {
    const step = stepAnnouncer(this.announcer, this.clock.now());
    this.announcer = step.state;
    if (step.announcement !== null) {
      this.update({ announcement: step.announcement });
    }
    if (step.waitMs !== null && this.announceTimer === null) {
      this.announceTimer = setTimeout(() => {
        this.announceTimer = null;
        this.flushAnnouncer();
      }, step.waitMs);
    }
  }
}

const stores = new Map<string, LiveStore>();

export function getLiveStore(endpointId: string): LiveStore {
  const existing = stores.get(endpointId);
  if (existing !== undefined) {
    return existing;
  }
  const created = new LiveStore();
  stores.set(endpointId, created);
  return created;
}

export function resetLiveStores(): void {
  for (const store of stores.values()) {
    store.dispose();
  }
  stores.clear();
}
