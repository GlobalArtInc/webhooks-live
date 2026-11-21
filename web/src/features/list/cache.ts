import type { QueryClient } from '@tanstack/react-query';
import { queryKeys, removeCaptureFromCache, type CaptureListData } from '../../api/queries';
import type { CaptureSummary, Endpoint } from '../../api/types';
import { restoreCaptureInPages } from './listModel';

export function withCaptureCount(endpoint: Endpoint, delta: number): Endpoint {
  return { ...endpoint, capture_count: Math.max(0, endpoint.capture_count + delta) };
}

function adjustCaptureCount(queryClient: QueryClient, endpointId: string, delta: number): void {
  queryClient.setQueryData<Endpoint>(queryKeys.endpoint(endpointId), (endpoint) =>
    endpoint === undefined ? undefined : withCaptureCount(endpoint, delta),
  );
}

export function hideCapture(queryClient: QueryClient, endpointId: string, captureId: string): void {
  removeCaptureFromCache(queryClient, endpointId, captureId);
  adjustCaptureCount(queryClient, endpointId, -1);
}

export function showCapture(queryClient: QueryClient, endpointId: string, capture: CaptureSummary): void {
  queryClient.setQueriesData<CaptureListData>({ queryKey: queryKeys.captureLists(endpointId) }, (data) =>
    data === undefined ? undefined : restoreCaptureInPages(data, capture),
  );
  adjustCaptureCount(queryClient, endpointId, 1);
}
