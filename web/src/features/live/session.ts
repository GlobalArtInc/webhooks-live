import type { QueryClient } from '@tanstack/react-query';
import { queryKeys } from '../../api/queries';
import type { CaptureSummary } from '../../api/types';
import { applyLiveEventToCache, type CatchUpReason, type LiveEffect } from './applyLiveEvent';
import {
  endpointCapacity,
  hasListData,
  highestHeldSeq,
  primaryListTotal,
  runCatchUp,
  suggestsMissedDeletes,
  type PageLoader,
} from './catchUp';
import type { LiveStore } from './store';
import type { LiveEvent, LiveHandler, LiveReadyPayload, LiveSourceEventName } from './types';

export interface LiveSourceLike {
  on<K extends LiveSourceEventName>(name: K, handler: LiveHandler<K>): () => void;
  start(): void;
  close(): void;
}

export interface SessionContext {
  hidden: boolean;
  following: boolean;
}

export interface LiveSessionOptions {
  queryClient: QueryClient;
  endpointId: string;
  source: LiveSourceLike;
  store: LiveStore;
  getContext: () => SessionContext;
  onFollow: (captureId: string) => void;
  loadPage?: PageLoader;
  now?: () => number;
}

export class LiveSession {
  private readonly options: LiveSessionOptions;
  private readonly unsubscribers: (() => void)[] = [];
  private readonly now: () => number;
  private ready: LiveReadyPayload | null = null;
  private deltaSinceReady = 0;
  private running = false;
  private rerun: CatchUpReason | null = null;
  private awaitingBaseline = false;
  private seenSeq = 0;
  private syncedSeq: number | null = null;
  private disposed = false;

  constructor(options: LiveSessionOptions) {
    this.options = options;
    this.now = options.now ?? Date.now;
  }

  start(): void {
    const { source, store } = this.options;
    this.unsubscribers.push(
      source.on('state', ({ state, reason }) => {
        store.setConnection(state, reason);
      }),
      source.on('invalid', () => {
        this.requestCatchUp('invalid');
      }),
      source.on('ready', (data) => {
        this.handle({ name: 'ready', data });
      }),
      source.on('capture', (data) => {
        this.handle({ name: 'capture', data });
      }),
      source.on('capture_deleted', (data) => {
        this.handle({ name: 'capture_deleted', data });
      }),
      source.on('captures_cleared', (data) => {
        this.handle({ name: 'captures_cleared', data });
      }),
      source.on('endpoint_updated', (data) => {
        this.handle({ name: 'endpoint_updated', data });
      }),
      source.on('forward_result', (data) => {
        this.handle({ name: 'forward_result', data });
      }),
      source.on('resync', (data) => {
        this.handle({ name: 'resync', data });
      }),
      source.on('endpoint_deleted', (data) => {
        this.handle({ name: 'endpoint_deleted', data });
      }),
      this.watchLists(),
    );
    source.start();
  }

  dispose(): void {
    if (this.disposed) {
      return;
    }
    this.disposed = true;
    for (const unsubscribe of this.unsubscribers.splice(0)) {
      unsubscribe();
    }
    this.options.source.close();
  }

  private watchLists(): () => void {
    const { queryClient, endpointId } = this.options;
    const prefix = queryKeys.captureLists(endpointId);
    return queryClient.getQueryCache().subscribe((event) => {
      if (event.type !== 'updated' || event.action.type !== 'success' || event.action.manual === true) {
        return;
      }
      const matches = prefix.every((part, index) => event.query.queryKey[index] === part);
      if (!matches) {
        return;
      }
      const behind = this.seenSeq > highestHeldSeq(queryClient, endpointId);
      if (this.awaitingBaseline || behind) {
        this.awaitingBaseline = false;
        this.requestCatchUp('resync');
      }
    });
  }

  private handle(event: LiveEvent): void {
    if (this.disposed) {
      return;
    }
    this.options.store.touch(this.now());
    const effects = applyLiveEventToCache(this.options.queryClient, this.options.endpointId, event);
    this.process(effects);
  }

  private process(effects: readonly LiveEffect[]): void {
    const inserted: CaptureSummary[] = [];
    for (const effect of effects) {
      switch (effect.kind) {
        case 'inserted':
          inserted.push(effect.capture);
          break;
        case 'removed':
          this.options.store.forget(effect.captureId);
          this.deltaSinceReady -= 1;
          break;
        case 'cleared':
          this.options.store.reset();
          break;
        case 'catchUp':
          if (effect.ready !== null) {
            this.ready = effect.ready;
            this.deltaSinceReady = 0;
          }
          this.requestCatchUp(effect.reason);
          break;
        case 'hidden':
          break;
      }
    }
    this.arrived(inserted);
  }

  private arrived(captures: readonly CaptureSummary[]): void {
    if (captures.length === 0) {
      return;
    }
    const readySeq = this.ready?.last_seq ?? 0;
    let newest = captures[0];
    for (const capture of captures) {
      this.seenSeq = Math.max(this.seenSeq, capture.seq);
      if (capture.seq > readySeq) {
        this.deltaSinceReady += 1;
      }
      if (newest === undefined || capture.seq > newest.seq) {
        newest = capture;
      }
    }
    const context = this.options.getContext();
    this.options.store.recordArrivals(captures, context);
    if (context.following && newest !== undefined) {
      this.options.onFollow(newest.id);
    }
  }

  private requestCatchUp(reason: CatchUpReason): void {
    if (this.disposed) {
      return;
    }
    if (this.running) {
      this.rerun = reason;
      return;
    }
    this.running = true;
    void this.drain(reason);
  }

  private async drain(first: CatchUpReason): Promise<void> {
    let reason: CatchUpReason | null = first;
    while (reason !== null && !this.disposed) {
      this.rerun = null;
      await this.catchUpOnce(reason);
      reason = this.rerun;
    }
    this.running = false;
  }

  private async catchUpOnce(reason: CatchUpReason): Promise<void> {
    const { queryClient, endpointId } = this.options;
    if (!hasListData(queryClient, endpointId)) {
      this.awaitingBaseline = true;
      return;
    }
    const held = Math.max(highestHeldSeq(queryClient, endpointId), this.seenSeq);
    const afterSeq = reason === 'resync' && this.syncedSeq !== null ? Math.min(this.syncedSeq, held) : held;
    let complete = false;
    try {
      const result = await runCatchUp(queryClient, endpointId, {
        afterSeq,
        loadPage: this.options.loadPage,
        isCancelled: () => this.disposed,
      });
      if (this.disposed) {
        return;
      }
      this.process(result.effects);
      complete = result.complete;
      if (result.complete) {
        this.syncedSeq = result.highest;
      }
      if (result.effects.some((effect) => effect.kind === 'inserted')) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.endpoint(endpointId) });
      }
    } catch {
      complete = false;
    }
    if (this.disposed) {
      return;
    }
    if (!complete || (reason === 'ready' && this.ready !== null && this.looksStale(this.ready))) {
      this.refetchLists();
    }
  }

  private looksStale(ready: LiveReadyPayload): boolean {
    const { queryClient, endpointId } = this.options;
    return suggestsMissedDeletes({
      readyCount: ready.capture_count,
      deltaSinceReady: this.deltaSinceReady,
      total: primaryListTotal(queryClient, endpointId),
      capacity: endpointCapacity(queryClient, endpointId),
    });
  }

  private refetchLists(): void {
    const { queryClient, endpointId } = this.options;
    void queryClient.invalidateQueries({ queryKey: queryKeys.captureLists(endpointId) });
    void queryClient.invalidateQueries({ queryKey: queryKeys.endpoint(endpointId) });
  }
}
