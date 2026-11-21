import { useInfiniteQuery, useMutation, useQueryClient, type QueryKey } from '@tanstack/react-query';
import { deleteCapture, listInboxCaptures } from '../../api/client';
import { removeCaptureFromCache, shouldRetryRequest } from '../../api/queries';
import type { CaptureSummary, InboxCaptureQuery, Page } from '../../api/types';
import { inboxListKey, removeInboxCapture, type InboxListData } from './cache';

const EMPTY_PAGE: Page<CaptureSummary> = { items: [], next_cursor: null };

export interface DeleteInboxCaptureVariables {
  endpointId: string;
  captureId: string;
}

export function useInboxCaptures(scopeKey: string, query: InboxCaptureQuery | null) {
  return useInfiniteQuery<Page<CaptureSummary>, Error, InboxListData, QueryKey, string | null>({
    queryKey: inboxListKey(scopeKey),
    queryFn: ({ pageParam }) =>
      query === null ? Promise.resolve(EMPTY_PAGE) : listInboxCaptures({ ...query, cursor: pageParam }),
    enabled: query !== null,
    initialPageParam: null,
    getNextPageParam: (lastPage) => lastPage.next_cursor,
    retry: shouldRetryRequest,
  });
}

export function useDeleteInboxCapture(scopeKey: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ endpointId, captureId }: DeleteInboxCaptureVariables) => deleteCapture(endpointId, captureId),
    onSuccess: (_result, { endpointId, captureId }) => {
      queryClient.setQueryData<InboxListData>(inboxListKey(scopeKey), (data) =>
        data === undefined ? data : removeInboxCapture(data, captureId),
      );
      removeCaptureFromCache(queryClient, endpointId, captureId);
    },
  });
}
