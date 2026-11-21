import { hashKey, type QueryClient, type QueryKey } from '@tanstack/react-query';
import type { CaptureSummary } from '../../api/types';
import type { LiveSourceLike } from '../live/session';
import { mergeInboxCapture, removeInboxCapture, type InboxListData } from './cache';

export interface InboxSessionOptions {
  queryClient: QueryClient;
  queryKey: QueryKey;
  source: LiveSourceLike;
  onActivity: () => void;
}

export class InboxSession {
  private readonly options: InboxSessionOptions;
  private readonly keyHash: string;
  private readonly unsubscribers: (() => void)[] = [];
  private awaitingFetch = false;
  private disposed = false;

  constructor(options: InboxSessionOptions) {
    this.options = options;
    this.keyHash = hashKey(options.queryKey);
  }

  start(): void {
    const { source } = this.options;
    this.unsubscribers.push(
      source.on('state', (change) => {
        if (change.state === 'open') {
          this.refetch();
        }
      }),
      source.on('capture', (capture) => {
        this.arrive(capture);
      }),
      source.on('capture_deleted', (payload) => {
        this.remove(payload.capture_id);
      }),
      source.on('captures_cleared', () => {
        this.refetch();
      }),
      source.on('endpoint_deleted', () => {
        this.refetch();
      }),
      source.on('resync', () => {
        this.refetch();
      }),
      this.watchFetches(),
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

  private arrive(capture: CaptureSummary): void {
    if (this.disposed) {
      return;
    }
    const { queryClient, queryKey } = this.options;
    this.options.onActivity();
    if (queryClient.isFetching({ queryKey }) > 0) {
      this.awaitingFetch = true;
    }
    queryClient.setQueryData<InboxListData>(queryKey, (data) =>
      data === undefined ? data : mergeInboxCapture(data, capture),
    );
  }

  private remove(captureId: string): void {
    if (this.disposed) {
      return;
    }
    this.options.queryClient.setQueryData<InboxListData>(this.options.queryKey, (data) =>
      data === undefined ? data : removeInboxCapture(data, captureId),
    );
  }

  private refetch(): void {
    if (this.disposed) {
      return;
    }
    void this.options.queryClient.invalidateQueries({ queryKey: this.options.queryKey, refetchType: 'active' });
  }

  private watchFetches(): () => void {
    return this.options.queryClient.getQueryCache().subscribe((event) => {
      if (event.type !== 'updated' || event.action.type !== 'success' || event.action.manual === true) {
        return;
      }
      if (!this.awaitingFetch || hashKey(event.query.queryKey) !== this.keyHash) {
        return;
      }
      this.awaitingFetch = false;
      this.refetch();
    });
  }
}
