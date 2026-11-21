import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useEffectEvent, useRef, useState } from 'react';
import { listCaptures } from '../../api/client';
import { queryKeys, shouldRetryRequest } from '../../api/queries';
import type { CaptureSummary } from '../../api/types';
import { fetchVariableCatalogue, renderPreview, sendSampleRequest } from './api';
import { INITIAL_PREVIEW_STATE, PreviewRunner } from './previewRunner';
import type { PreviewRequest, PreviewState } from './types';

export const PICKER_LIMIT = 20;
export const CAPTURE_POLL_MS = 5000;
const CATALOGUE_STALE_MS = 30_000;

function previewSourcesKey(endpointId: string) {
  return [...queryKeys.captures(endpointId), 'preview-sources'] as const;
}

export function useVariableCatalogue(endpointId: string, captureId: string | null, enabled: boolean) {
  return useQuery({
    queryKey: ['template', endpointId, 'variables', captureId ?? 'none'],
    queryFn: ({ signal }) => fetchVariableCatalogue(endpointId, captureId, signal),
    enabled,
    staleTime: CATALOGUE_STALE_MS,
    retry: shouldRetryRequest,
  });
}

export function usePreviewCaptures(endpointId: string, enabled: boolean) {
  return useQuery({
    queryKey: previewSourcesKey(endpointId),
    queryFn: async (): Promise<CaptureSummary[]> => {
      const page = await listCaptures(endpointId, { limit: PICKER_LIMIT, order: 'desc' });
      return page.items;
    },
    enabled,
    refetchInterval: CAPTURE_POLL_MS,
    retry: shouldRetryRequest,
  });
}

export function useSendSample(endpointId: string, endpointUrl: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (): Promise<string | null> => {
      await sendSampleRequest(endpointUrl);
      const page = await listCaptures(endpointId, { limit: 1, order: 'desc' });
      return page.items[0]?.id ?? null;
    },
    onSuccess: () => queryClient.invalidateQueries({ queryKey: previewSourcesKey(endpointId) }),
  });
}

export interface TemplatePreviewOptions {
  endpointId: string;
  request: PreviewRequest | null;
  enabled: boolean;
}

export interface TemplatePreview {
  state: PreviewState;
  requestKey: string | null;
  stale: boolean;
  refresh: () => void;
}

export function useTemplatePreview({ endpointId, request, enabled }: TemplatePreviewOptions): TemplatePreview {
  const [state, setState] = useState<PreviewState>(INITIAL_PREVIEW_STATE);
  const runnerRef = useRef<PreviewRunner | null>(null);
  const previousDraftRef = useRef<string | null>(null);
  const requestKey = request === null ? null : JSON.stringify(request);
  const draftKey = request === null ? null : JSON.stringify(request.response);
  const currentRequest = useEffectEvent(() => request);

  useEffect(() => {
    const runner = new PreviewRunner((payload, signal) => renderPreview(endpointId, payload, signal), setState);
    runnerRef.current = runner;
    return () => {
      runner.dispose();
      runnerRef.current = null;
      previousDraftRef.current = null;
    };
  }, [endpointId]);

  useEffect(() => {
    const runner = runnerRef.current;
    const next = currentRequest();
    if (runner === null) {
      return;
    }
    if (!enabled || next === null || requestKey === null) {
      runner.cancel();
      previousDraftRef.current = null;
      return;
    }
    const immediate = previousDraftRef.current === null || previousDraftRef.current === draftKey;
    previousDraftRef.current = draftKey;
    runner.schedule(next, requestKey, immediate);
  }, [enabled, requestKey, draftKey, endpointId]);

  const refresh = useCallback(() => {
    if (request !== null && requestKey !== null) {
      runnerRef.current?.runNow(request, requestKey);
    }
  }, [request, requestKey]);

  const stale = state.result !== null && state.resultKey !== requestKey;
  return { state, requestKey, stale, refresh };
}
