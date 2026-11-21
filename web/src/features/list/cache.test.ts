import { QueryClient } from '@tanstack/react-query';
import { describe, expect, it } from 'vitest';
import { queryKeys, type CaptureListData } from '../../api/queries';
import type { CapturePage, Endpoint } from '../../api/types';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { captureIdFor, summaryAt } from '../endpoint/testFixtures';
import { hideCapture, showCapture, withCaptureCount } from './cache';
import { CAPTURE_LIST_FILTERS } from './useCaptureList';

function pageOf(seqs: number[], total: number): CapturePage {
  return { items: seqs.map(summaryAt), next_cursor: null, total };
}

function seedClient(): QueryClient {
  const client = new QueryClient();
  const endpoint: Endpoint = { ...endpointFixture, capture_count: 3, last_seq: 3 };
  client.setQueryData<Endpoint>(queryKeys.endpoint(ENDPOINT_ID), endpoint);
  const data: CaptureListData = { pages: [pageOf([3, 2, 1], 3)], pageParams: [null] };
  client.setQueryData<CaptureListData>(queryKeys.captureList(ENDPOINT_ID, CAPTURE_LIST_FILTERS), data);
  return client;
}

function listedSeqs(client: QueryClient): number[] {
  const data = client.getQueryData<CaptureListData>(queryKeys.captureList(ENDPOINT_ID, CAPTURE_LIST_FILTERS));
  return data?.pages.flatMap((page) => page.items.map((item) => item.seq)) ?? [];
}

describe('optimistic capture cache writes', () => {
  it('hides a capture and lowers the endpoint count, then shows it again at its sorted place', () => {
    const client = seedClient();

    hideCapture(client, ENDPOINT_ID, captureIdFor(2));
    expect(listedSeqs(client)).toEqual([3, 1]);
    expect(client.getQueryData<Endpoint>(queryKeys.endpoint(ENDPOINT_ID))?.capture_count).toBe(2);

    showCapture(client, ENDPOINT_ID, summaryAt(2));
    expect(listedSeqs(client)).toEqual([3, 2, 1]);
    expect(client.getQueryData<Endpoint>(queryKeys.endpoint(ENDPOINT_ID))?.capture_count).toBe(3);
  });

  it('never counts a capture below zero', () => {
    expect(withCaptureCount({ ...endpointFixture, capture_count: 0 }, -1).capture_count).toBe(0);
    expect(withCaptureCount({ ...endpointFixture, capture_count: 4 }, 1).capture_count).toBe(5);
  });
});
