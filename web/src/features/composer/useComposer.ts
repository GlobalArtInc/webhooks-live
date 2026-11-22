import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { deleteSentRequest, getCapture, listSentRequests, sendRequest } from '../../api/client';
import { queryKeys, shouldRetryRequest } from '../../api/queries';
import type { SendRequest } from '../../api/types';

const HISTORY_LIMIT = 20;

export const composerHistoryKey = ['composer', 'history'] as const;

export interface SendVariables {
  input: SendRequest;
  signal: AbortSignal;
}

export function useSentHistory(endpointId: string | null, enabled: boolean) {
  return useQuery({
    queryKey: [...composerHistoryKey, endpointId ?? ''],
    queryFn: () =>
      listSentRequests(
        endpointId === null ? { limit: HISTORY_LIMIT } : { endpoint_id: endpointId, limit: HISTORY_LIMIT },
      ),
    retry: shouldRetryRequest,
    enabled,
  });
}

export function useSendComposerRequest() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ input, signal }: SendVariables) => sendRequest(input, signal),
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: composerHistoryKey });
    },
  });
}

export function useDeleteSentRequest() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (sentId: string) => deleteSentRequest(sentId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: composerHistoryKey });
    },
  });
}

export function useReplaySource(endpointId: string | undefined, captureId: string | undefined) {
  const ready = endpointId !== undefined && captureId !== undefined;
  return useQuery({
    queryKey: queryKeys.capture(endpointId ?? '', captureId ?? ''),
    queryFn: () => getCapture(endpointId ?? '', captureId ?? ''),
    enabled: ready,
    retry: shouldRetryRequest,
  });
}
