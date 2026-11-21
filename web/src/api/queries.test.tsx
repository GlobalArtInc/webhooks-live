import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, renderHook, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { describe, expect, it } from 'vitest';
import {
  binaryCaptureFixture,
  captureFixtures,
  formCaptureFixture,
  forwardedCaptureFixture,
  jsonCaptureFixture,
} from '../test/fixtures/captures';
import { ENDPOINT_ID, endpointFixture } from '../test/fixtures/endpoint';
import { registerMockServer } from '../test/msw/server';
import { seedEndpoint } from '../test/msw/store';
import { ApiError } from './client';
import {
  type CaptureListData,
  mergeCaptureIntoCache,
  queryKeys,
  useCapture,
  useCaptures,
  useClearCaptures,
  useCreateEndpoint,
  useDeleteCapture,
  useEndpoint,
} from './queries';
import type { CaptureSummary, Endpoint } from './types';

const LIVE_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a09';

registerMockServer();

interface WrapperProps {
  children: ReactNode;
}

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: WrapperProps) {
    return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  };
}

function captureIds(data: CaptureListData | undefined): string[] {
  return data?.pages.flatMap((page) => page.items.map((item) => item.id)) ?? [];
}

const NEWEST_FIRST_IDS = [
  forwardedCaptureFixture.id,
  binaryCaptureFixture.id,
  formCaptureFixture.id,
  jsonCaptureFixture.id,
];

describe('query hooks', () => {
  it('pages captures newest first and continues from the cursor', async () => {
    seedEndpoint(endpointFixture, captureFixtures);
    const { result } = renderHook(
      () => {
        const query = useCaptures(ENDPOINT_ID, { limit: 2 });
        return {
          query,
          ids: captureIds(query.data),
          isSuccess: query.isSuccess,
          hasNextPage: query.hasNextPage,
        };
      },
      { wrapper: createWrapper(new QueryClient()) },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.ids).toEqual(NEWEST_FIRST_IDS.slice(0, 2));
    expect(result.current.hasNextPage).toBe(true);

    await act(() => result.current.query.fetchNextPage());

    await waitFor(() => expect(result.current.ids).toEqual(NEWEST_FIRST_IDS));
    expect(result.current.hasNextPage).toBe(false);
  });

  it('loads a capture detail with its body and forward jobs', async () => {
    seedEndpoint(endpointFixture, captureFixtures);
    const { result } = renderHook(
      () => {
        const query = useCapture(ENDPOINT_ID, forwardedCaptureFixture.id);
        return { data: query.data, isSuccess: query.isSuccess };
      },
      { wrapper: createWrapper(new QueryClient()) },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(forwardedCaptureFixture);
  });

  it('creates an endpoint and stores it under its id', async () => {
    const queryClient = new QueryClient();
    const { result } = renderHook(() => useCreateEndpoint(), { wrapper: createWrapper(queryClient) });

    const created = await act(() => result.current.mutateAsync({ title: 'new endpoint' }));

    expect(created.title).toBe('new endpoint');
    expect(created.id).toMatch(/^[0-9a-hjkmnp-tv-z]{24}$/);
    expect(queryClient.getQueryData<Endpoint>(queryKeys.endpoint(created.id))).toEqual(created);
  });

  it('deletes a capture and removes it from the cached pages', async () => {
    seedEndpoint(endpointFixture, captureFixtures);
    const { result } = renderHook(
      () => {
        const captures = useCaptures(ENDPOINT_ID);
        const remove = useDeleteCapture(ENDPOINT_ID);
        return { isSuccess: captures.isSuccess, ids: captureIds(captures.data), remove: remove.mutateAsync };
      },
      { wrapper: createWrapper(new QueryClient()) },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    await act(() => result.current.remove(jsonCaptureFixture.id));

    await waitFor(() => expect(result.current.ids).not.toContain(jsonCaptureFixture.id));
    expect(result.current.ids).toHaveLength(captureFixtures.length - 1);
  });

  it('clears all captures and empties the cached pages and endpoint count', async () => {
    seedEndpoint(endpointFixture, captureFixtures);
    const { result } = renderHook(
      () => {
        const captures = useCaptures(ENDPOINT_ID);
        const endpoint = useEndpoint(ENDPOINT_ID);
        const clear = useClearCaptures(ENDPOINT_ID);
        return {
          isSuccess: captures.isSuccess,
          ids: captureIds(captures.data),
          captureCount: endpoint.data?.capture_count,
          clear: clear.mutateAsync,
        };
      },
      { wrapper: createWrapper(new QueryClient()) },
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await waitFor(() => expect(result.current.captureCount).toBe(captureFixtures.length));

    await act(() => result.current.clear());

    await waitFor(() => expect(result.current.ids).toEqual([]));
    expect(result.current.captureCount).toBe(0);
  });

  it('merges a live capture into the cached list and counts it on the endpoint', async () => {
    seedEndpoint(endpointFixture, captureFixtures);
    const queryClient = new QueryClient();
    const { result } = renderHook(
      () => {
        const captures = useCaptures(ENDPOINT_ID);
        const endpoint = useEndpoint(ENDPOINT_ID);
        return {
          isReady: captures.isSuccess && endpoint.isSuccess,
          ids: captureIds(captures.data),
          captureCount: endpoint.data?.capture_count,
        };
      },
      { wrapper: createWrapper(queryClient) },
    );
    await waitFor(() => expect(result.current.isReady).toBe(true));
    const live: CaptureSummary = { ...jsonCaptureFixture, id: LIVE_CAPTURE_ID, seq: captureFixtures.length + 1 };

    act(() => {
      mergeCaptureIntoCache(queryClient, ENDPOINT_ID, live);
    });

    await waitFor(() => expect(result.current.ids[0]).toBe(LIVE_CAPTURE_ID));
    expect(result.current.captureCount).toBe(captureFixtures.length + 1);
  });

  it('does not retry a not found answer', async () => {
    const { result } = renderHook(
      () => {
        const query = useEndpoint('zzzzzzzzzzzzzzzzzzzzzzzz');
        return { isError: query.isError, error: query.error, failureCount: query.failureCount };
      },
      { wrapper: createWrapper(new QueryClient()) },
    );

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error).toBeInstanceOf(ApiError);
    expect(result.current.failureCount).toBe(1);
  });
});
