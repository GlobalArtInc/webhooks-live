import { API_PREFIX, GRANT_HEADER, reportGrantRejected } from '../../api/client';
import { GRANT_REQUIRED_CODE } from '../access/grantRequired';
import { clearGrant, readGrant } from '../access/grants';
import { BYE_LOOP_GUARD_MS, HIDDEN_GRACE_MS, STABLE_CONNECTION_MS, backoffDelay } from './backoff';
import { parseLiveEvent } from './parse';
import {
  LIVE_EVENT_NAMES,
  type ConnectionState,
  type EndReason,
  type LiveEvent,
  type LiveEventName,
  type LiveHandler,
  type LiveSourceEventName,
  type LiveSourceEvents,
} from './types';

const READY_STATE_CLOSED = 2;
const HTTP_GONE = 410;
const HTTP_NOT_FOUND = 404;
const HTTP_UNAUTHORIZED = 401;
const UNKNOWN_STATUS = 0;

type HandlerSets = { [K in LiveSourceEventName]: Set<LiveHandler<K>> };
type Timer = ReturnType<typeof setTimeout>;

export interface EventSourceLike {
  readonly readyState: number;
  close(): void;
  addEventListener(type: string, listener: (event: Event) => void): void;
}

export type EventSourceFactory = (url: string) => EventSourceLike;
export type StatusProbe = (endpointId: string) => Promise<number>;

export interface LiveSourceOptions {
  endpointId?: string;
  getLastSeq?: () => number | null;
  createEventSource?: EventSourceFactory | null;
  probe?: StatusProbe;
  random?: () => number;
  now?: () => number;
  buildUrl?: (lastEventId: string | null) => string;
  finishOnEndpointDeleted?: boolean;
}

export function streamUrl(endpointId: string, lastSeq: number): string {
  const base = `${API_PREFIX}/endpoints/${encodeURIComponent(endpointId)}/stream`;
  const params = new URLSearchParams();
  if (lastSeq > 0) {
    params.set('last_event_id', String(lastSeq));
  }
  const grant = readGrant(endpointId);
  if (grant !== null) {
    params.set('grant', grant);
  }
  const query = params.toString();
  return query === '' ? base : `${base}?${query}`;
}

function browserEventSource(): EventSourceFactory | null {
  return typeof EventSource === 'undefined' ? null : (url) => new EventSource(url);
}

async function fetchEndpointStatus(endpointId: string): Promise<number> {
  const grant = readGrant(endpointId);
  const response = await fetch(`${API_PREFIX}/endpoints/${encodeURIComponent(endpointId)}`, {
    method: 'GET',
    credentials: 'same-origin',
    headers: grant === null ? undefined : { [GRANT_HEADER]: grant },
  });
  if (response.status === HTTP_UNAUTHORIZED) {
    return (await isGrantRequiredBody(response)) ? HTTP_UNAUTHORIZED : UNKNOWN_STATUS;
  }
  return response.status;
}

async function isGrantRequiredBody(response: Response): Promise<boolean> {
  const body: unknown = await response.json().catch(() => null);
  if (typeof body !== 'object' || body === null || !('error' in body)) {
    return false;
  }
  const { error } = body;
  return typeof error === 'object' && error !== null && 'code' in error && error.code === GRANT_REQUIRED_CODE;
}

function createHandlerSets(): HandlerSets {
  return {
    ready: new Set(),
    capture: new Set(),
    capture_deleted: new Set(),
    captures_cleared: new Set(),
    endpoint_updated: new Set(),
    endpoint_deleted: new Set(),
    forward_result: new Set(),
    resync: new Set(),
    bye: new Set(),
    state: new Set(),
    invalid: new Set(),
  };
}

export class LiveSource {
  private readonly options: LiveSourceOptions;
  private readonly factory: EventSourceFactory | null;
  private readonly probe: StatusProbe;
  private readonly random: () => number;
  private readonly now: () => number;
  private readonly handlers = createHandlerSets();
  private state: ConnectionState = 'connecting';
  private reason: EndReason | null = null;
  private source: EventSourceLike | null = null;
  private retryTimer: Timer | null = null;
  private hiddenTimer: Timer | null = null;
  private attempt = 0;
  private openedAt: number | null = null;
  private hasConnected = false;
  private suspended = false;
  private disposed = false;
  private started = false;
  private highSeq = 0;
  private lastEventId: string | null = null;
  private probeToken = 0;

  constructor(options: LiveSourceOptions) {
    this.options = options;
    this.factory = options.createEventSource === undefined ? browserEventSource() : options.createEventSource;
    this.probe = options.probe ?? fetchEndpointStatus;
    this.random = options.random ?? Math.random;
    this.now = options.now ?? Date.now;
  }

  on<K extends LiveSourceEventName>(name: K, handler: LiveHandler<K>): () => void {
    const set: Set<LiveHandler<K>> = this.handlers[name];
    set.add(handler);
    return () => {
      set.delete(handler);
    };
  }

  getState(): ConnectionState {
    return this.state;
  }

  getEndReason(): EndReason | null {
    return this.reason;
  }

  getLastSeq(): number {
    return Math.max(this.highSeq, this.options.getLastSeq?.() ?? 0);
  }

  start(): void {
    if (this.started || this.disposed) {
      return;
    }
    this.started = true;
    document.addEventListener('visibilitychange', this.handleVisibility);
    window.addEventListener('online', this.handleOnline);
    window.addEventListener('offline', this.handleOffline);
    this.connect();
    if (document.hidden) {
      this.armHiddenTimer();
    }
  }

  close(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    document.removeEventListener('visibilitychange', this.handleVisibility);
    window.removeEventListener('online', this.handleOnline);
    window.removeEventListener('offline', this.handleOffline);
    this.clearTimers();
    this.closeSource();
  }

  private emit<K extends LiveSourceEventName>(name: K, payload: LiveSourceEvents[K]): void {
    const set: Set<LiveHandler<K>> = this.handlers[name];
    for (const handler of [...set]) {
      handler(payload);
    }
  }

  private setState(state: ConnectionState, reason: EndReason | null = null): void {
    if (this.state === state && this.reason === reason) {
      return;
    }
    this.state = state;
    this.reason = reason;
    this.emit('state', { state, reason });
  }

  private clearTimers(): void {
    if (this.retryTimer !== null) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
    if (this.hiddenTimer !== null) {
      clearTimeout(this.hiddenTimer);
      this.hiddenTimer = null;
    }
    this.probeToken += 1;
  }

  private closeSource(): void {
    const source = this.source;
    this.source = null;
    source?.close();
  }

  private connect(): void {
    if (this.disposed || this.state === 'ended') {
      return;
    }
    if (this.retryTimer !== null) {
      clearTimeout(this.retryTimer);
      this.retryTimer = null;
    }
    this.closeSource();
    this.probeToken += 1;
    this.suspended = false;
    if (this.factory === null) {
      this.setState('offline');
      return;
    }
    this.setState(this.hasConnected || this.attempt > 0 ? 'reconnecting' : 'connecting');
    const source = this.factory(this.currentUrl());
    this.source = source;
    source.addEventListener('open', () => {
      if (this.source !== source) {
        return;
      }
      this.hasConnected = true;
      this.openedAt = this.now();
      this.setState('open');
    });
    source.addEventListener('error', () => {
      this.handleError(source);
    });
    for (const name of LIVE_EVENT_NAMES) {
      source.addEventListener(name, (event) => {
        this.handleMessage(source, name, event);
      });
    }
  }

  private currentUrl(): string {
    const { buildUrl, endpointId } = this.options;
    return buildUrl === undefined ? streamUrl(endpointId ?? '', this.getLastSeq()) : buildUrl(this.lastEventId);
  }

  private handleMessage(source: EventSourceLike, name: LiveEventName, event: Event): void {
    if (this.source !== source || !(event instanceof MessageEvent)) {
      return;
    }
    if (event.lastEventId !== '') {
      this.lastEventId = event.lastEventId;
    }
    const parsed = parseLiveEvent(name, String(event.data));
    if (parsed === null) {
      this.emit('invalid', { name });
      return;
    }
    if (parsed.name === 'capture') {
      this.highSeq = Math.max(this.highSeq, parsed.data.seq);
    }
    this.dispatch(parsed);
    if (parsed.name === 'bye') {
      this.handleBye();
    } else if (parsed.name === 'endpoint_deleted' && this.options.finishOnEndpointDeleted !== false) {
      this.finish('deleted');
    }
  }

  private dispatch(event: LiveEvent): void {
    this.emit(event.name, event.data);
  }

  private settleStability(): void {
    if (this.openedAt !== null && this.now() - this.openedAt >= STABLE_CONNECTION_MS) {
      this.attempt = 0;
    }
    this.openedAt = null;
  }

  private handleError(source: EventSourceLike): void {
    if (this.source !== source || this.disposed) {
      return;
    }
    const closedByBrowser = source.readyState === READY_STATE_CLOSED;
    this.closeSource();
    this.settleStability();
    if (document.hidden) {
      this.suspend();
      return;
    }
    if (navigator.onLine === false) {
      this.goOffline();
      return;
    }
    if (closedByBrowser) {
      void this.probeThenRetry();
      return;
    }
    this.scheduleRetry();
  }

  private handleBye(): void {
    this.closeSource();
    const lived = this.openedAt === null ? Infinity : this.now() - this.openedAt;
    this.openedAt = null;
    if (document.hidden) {
      this.suspend();
      return;
    }
    if (lived < BYE_LOOP_GUARD_MS) {
      this.scheduleRetry();
      return;
    }
    this.connect();
  }

  private async probeThenRetry(): Promise<void> {
    this.setState('reconnecting');
    this.probeToken += 1;
    const token = this.probeToken;
    let status: number | null = null;
    try {
      status = await this.probe(this.options.endpointId ?? '');
    } catch {
      status = null;
    }
    if (this.disposed || token !== this.probeToken || this.state === 'ended') {
      return;
    }
    if (status === HTTP_GONE) {
      this.finish('gone');
    } else if (status === HTTP_NOT_FOUND) {
      this.finish('missing');
    } else if (status === HTTP_UNAUTHORIZED) {
      this.finish('locked');
      const { endpointId } = this.options;
      if (endpointId !== undefined) {
        clearGrant(endpointId);
        reportGrantRejected(endpointId);
      }
    } else {
      this.scheduleRetry();
    }
  }

  private finish(reason: EndReason): void {
    this.clearTimers();
    this.closeSource();
    this.setState('ended', reason);
  }

  private scheduleRetry(): void {
    if (this.disposed || this.state === 'ended') {
      return;
    }
    if (document.hidden) {
      this.suspend();
      return;
    }
    const delay = backoffDelay(this.attempt, this.random());
    this.attempt += 1;
    this.setState('reconnecting');
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.connect();
    }, delay);
  }

  private goOffline(): void {
    this.clearTimers();
    this.closeSource();
    this.setState('offline');
  }

  private suspend(): void {
    if (this.state === 'ended') {
      return;
    }
    this.clearTimers();
    this.closeSource();
    this.suspended = true;
    this.setState('offline');
  }

  private armHiddenTimer(): void {
    if (this.hiddenTimer !== null || this.suspended || this.state === 'ended') {
      return;
    }
    this.hiddenTimer = setTimeout(() => {
      this.hiddenTimer = null;
      this.suspend();
    }, HIDDEN_GRACE_MS);
  }

  private reconnectNow(): void {
    if (this.disposed || this.state === 'ended') {
      return;
    }
    if (!this.suspended && (this.state === 'open' || this.state === 'connecting')) {
      return;
    }
    if (this.suspended) {
      this.attempt = 0;
    }
    this.connect();
  }

  private readonly handleVisibility = (): void => {
    if (document.hidden) {
      this.armHiddenTimer();
      return;
    }
    if (this.hiddenTimer !== null) {
      clearTimeout(this.hiddenTimer);
      this.hiddenTimer = null;
    }
    if (navigator.onLine === false) {
      return;
    }
    this.reconnectNow();
  };

  private readonly handleOnline = (): void => {
    if (document.hidden) {
      return;
    }
    this.reconnectNow();
  };

  private readonly handleOffline = (): void => {
    if (this.state === 'ended') {
      return;
    }
    this.goOffline();
  };
}
