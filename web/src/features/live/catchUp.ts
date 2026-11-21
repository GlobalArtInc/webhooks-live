import type { QueryClient } from '@tanstack/react-query';
import { queryKeys, type CaptureListData } from '../../api/queries';
import { listCaptures } from '../../api/client';
import type { CaptureListQuery, CapturePage, Endpoint } from '../../api/types';
import { flattenCaptures, totalOf } from '../list/listModel';
import { applyLiveEventToCache, type LiveEffect } from './applyLiveEvent';

export const CATCH_UP_PAGE_LIMIT = 200;
export const CATCH_UP_MAX_PAGES = 25;

export interface CatchUpQuery extends CaptureListQuery {
  after_seq: number;
}

export type PageLoader = (endpointId: string, query: CatchUpQuery) => Promise<CapturePage>;

export interface CatchUpOptions {
  afterSeq: number;
  loadPage?: PageLoader;
  isCancelled: () => boolean;
}

export interface CatchUpResult {
  effects: LiveEffect[];
  complete: boolean;
  highest: number;
}

export interface ReadyBaseline {
  readyCount: number;
  deltaSinceReady: number;
  total: number | null;
  capacity: number | undefined;
}

const loadFromApi: PageLoader = (endpointId, query) => listCaptures(endpointId, query);

export function listQueries(queryClient: QueryClient, endpointId: string): (CaptureListData | undefined)[] {
  return queryClient
    .getQueryCache()
    .findAll({ queryKey: queryKeys.captureLists(endpointId) })
    .map((query) => queryClient.getQueryData<CaptureListData>(query.queryKey));
}

export function hasListData(queryClient: QueryClient, endpointId: string): boolean {
  return listQueries(queryClient, endpointId).some((data) => data !== undefined);
}

export function highestHeldSeq(queryClient: QueryClient, endpointId: string): number {
  let highest = 0;
  for (const data of listQueries(queryClient, endpointId)) {
    for (const capture of flattenCaptures(data)) {
      highest = Math.max(highest, capture.seq);
    }
  }
  return highest;
}

export function primaryListTotal(queryClient: QueryClient, endpointId: string): number | null {
  const first = listQueries(queryClient, endpointId).find((data) => data !== undefined);
  return totalOf(first);
}

export function suggestsMissedDeletes(baseline: ReadyBaseline): boolean {
  if (baseline.total === null) {
    return false;
  }
  const expected = Math.max(0, baseline.readyCount + baseline.deltaSinceReady);
  const bounded = baseline.capacity === undefined ? expected : Math.min(baseline.capacity, expected);
  return baseline.total !== bounded;
}

export function endpointCapacity(queryClient: QueryClient, endpointId: string): number | undefined {
  return queryClient.getQueryData<Endpoint>(queryKeys.endpoint(endpointId))?.max_captures;
}

export async function runCatchUp(
  queryClient: QueryClient,
  endpointId: string,
  options: CatchUpOptions,
): Promise<CatchUpResult> {
  const loadPage = options.loadPage ?? loadFromApi;
  const effects: LiveEffect[] = [];
  let highest = options.afterSeq;
  let cursor: string | null = null;
  for (let pageCount = 0; pageCount < CATCH_UP_MAX_PAGES; pageCount += 1) {
    const page: CapturePage = await loadPage(endpointId, {
      order: 'asc',
      limit: CATCH_UP_PAGE_LIMIT,
      after_seq: options.afterSeq,
      cursor,
    });
    if (options.isCancelled()) {
      return { effects, complete: false, highest };
    }
    for (const capture of page.items) {
      highest = Math.max(highest, capture.seq);
      effects.push(...applyLiveEventToCache(queryClient, endpointId, { name: 'capture', data: capture }));
    }
    if (page.next_cursor === null) {
      return { effects, complete: true, highest };
    }
    cursor = page.next_cursor;
  }
  return { effects, complete: false, highest };
}
