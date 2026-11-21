import { QueryClient } from '@tanstack/react-query';
import { queryKeys, type CaptureListData } from '../../api/queries';
import type { CaptureSummary, Endpoint, ForwardJob } from '../../api/types';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { captureIdFor } from '../endpoint/testFixtures';
import { CAPTURE_LIST_FILTERS } from '../list';

export { ENDPOINT_ID };

const BASE_TIME_MS = Date.parse('2026-11-21T10:00:00.000Z');

export function summaryFor(seq: number, overrides: Partial<CaptureSummary> = {}): CaptureSummary {
  return {
    id: captureIdFor(seq),
    seq,
    endpoint_id: ENDPOINT_ID,
    received_at: new Date(BASE_TIME_MS + seq * 1000).toISOString(),
    method: 'POST',
    path: `/hook/${seq}`,
    query_string: '',
    client_ip: '203.0.113.7',
    content_type: 'application/json',
    body_size: seq,
    status_override: null,
    response_status: 200,
    forward_state: null,
    ...overrides,
  };
}

export function endpointWith(overrides: Partial<Endpoint> = {}): Endpoint {
  return { ...endpointFixture, ...overrides };
}

export function jobFor(state: ForwardJob['state'], id = 'job-1'): ForwardJob {
  return {
    id,
    kind: 'rule',
    rule_id: null,
    target_url: 'https://api.example.test/hooks',
    method: 'POST',
    state,
    attempts: [],
    created_at: '2026-11-21T10:00:00.000Z',
    finished_at: null,
  };
}

export interface ListShape {
  pageSize?: number;
  total?: number;
  nextCursor?: string | null;
}

export function listOf(seqs: readonly number[], shape: ListShape = {}): CaptureListData {
  const pageSize = shape.pageSize ?? Math.max(1, seqs.length);
  const total = shape.total ?? seqs.length;
  const pages = [];
  for (let start = 0; start < seqs.length || pages.length === 0; start += pageSize) {
    const last = start + pageSize >= seqs.length;
    pages.push({
      items: seqs.slice(start, start + pageSize).map((seq) => summaryFor(seq)),
      next_cursor: last ? (shape.nextCursor ?? null) : String(start + pageSize),
      total,
    });
  }
  return { pages, pageParams: pages.map((_page, index) => (index === 0 ? null : String(index * pageSize))) };
}

export function seqsOf(data: CaptureListData | undefined): number[] {
  return data === undefined ? [] : data.pages.flatMap((page) => page.items.map((item) => item.seq));
}

export function createSeededClient(seqs: readonly number[], endpoint: Partial<Endpoint> = {}, shape: ListShape = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
  queryClient.setQueryData(queryKeys.captureList(ENDPOINT_ID, CAPTURE_LIST_FILTERS), listOf(seqs, shape));
  queryClient.setQueryData(
    queryKeys.endpoint(ENDPOINT_ID),
    endpointWith({ capture_count: seqs.length, last_seq: Math.max(0, ...seqs), ...endpoint }),
  );
  return queryClient;
}

export function cachedList(queryClient: QueryClient): CaptureListData | undefined {
  return queryClient.getQueryData<CaptureListData>(queryKeys.captureList(ENDPOINT_ID, CAPTURE_LIST_FILTERS));
}

export function cachedEndpoint(queryClient: QueryClient): Endpoint | undefined {
  return queryClient.getQueryData<Endpoint>(queryKeys.endpoint(ENDPOINT_ID));
}
