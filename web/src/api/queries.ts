import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
  type QueryClient,
  type QueryKey,
} from '@tanstack/react-query';
import {
  ApiError,
  clearCaptures,
  createEndpoint,
  deleteCapture,
  deleteEndpoint,
  getCapture,
  getEndpoint,
  getInstance,
  listCaptures,
  patchEndpoint,
  resendCapture,
} from './client';
import type {
  CaptureListFilters,
  CaptureOrder,
  CapturePage,
  CaptureSummary,
  Endpoint,
  EndpointInput,
  ForwardRequest,
  ForwardRuleInput,
} from './types';

export type CaptureListData = InfiniteData<CapturePage, string | null>;

type CaptureListUpdate = (data: CaptureListData, order: CaptureOrder) => CaptureListData;
type EndpointUpdate = (endpoint: Endpoint) => Endpoint;

export interface ResendVariables {
  captureId: string;
  input: ForwardRequest;
}

const MAX_QUERY_RETRIES = 2;
const FIRST_PAGE_CURSOR: string | null = null;

export const queryKeys = {
  instance: () => ['instance'] as const,
  endpointLists: () => ['endpoint-list'] as const,
  endpoint: (endpointId: string) => ['endpoint', endpointId] as const,
  captures: (endpointId: string) => ['captures', endpointId] as const,
  captureLists: (endpointId: string) => ['captures', endpointId, 'list'] as const,
  captureList: (endpointId: string, filters: CaptureListFilters) => ['captures', endpointId, 'list', filters] as const,
  captureDetails: (endpointId: string) => ['captures', endpointId, 'detail'] as const,
  capture: (endpointId: string, captureId: string) => ['captures', endpointId, 'detail', captureId] as const,
};

export function shouldRetryRequest(failureCount: number, error: Error): boolean {
  if (failureCount >= MAX_QUERY_RETRIES) {
    return false;
  }
  return error instanceof ApiError ? error.status >= 500 : true;
}

function readCaptureOrder(queryKey: QueryKey): CaptureOrder {
  const filters = queryKey[3];
  return typeof filters === 'object' && filters !== null && 'order' in filters && filters.order === 'asc'
    ? 'asc'
    : 'desc';
}

function placeCapture(items: CaptureSummary[], capture: CaptureSummary, order: CaptureOrder): CaptureSummary[] {
  return order === 'desc' ? [capture, ...items] : [...items, capture];
}

export function mergeCaptureIntoPages(
  data: CaptureListData,
  capture: CaptureSummary,
  order: CaptureOrder,
): CaptureListData {
  const cached = data.pages.some((page) => page.items.some((item) => item.id === capture.id));
  if (cached) {
    return {
      ...data,
      pages: data.pages.map((page) => ({
        ...page,
        items: page.items.map((item) => (item.id === capture.id ? capture : item)),
      })),
    };
  }
  const target = order === 'desc' ? 0 : data.pages.length - 1;
  return {
    ...data,
    pages: data.pages.map((page, index) => ({
      ...page,
      items: index === target ? placeCapture(page.items, capture, order) : page.items,
      total: page.total + 1,
    })),
  };
}

export function removeCaptureFromPages(data: CaptureListData, captureId: string): CaptureListData {
  const cached = data.pages.some((page) => page.items.some((item) => item.id === captureId));
  if (!cached) {
    return data;
  }
  return {
    ...data,
    pages: data.pages.map((page) => ({
      ...page,
      items: page.items.filter((item) => item.id !== captureId),
      total: page.total - 1,
    })),
  };
}

export function clearCapturePages(): CaptureListData {
  return { pages: [{ items: [], next_cursor: null, total: 0 }], pageParams: [null] };
}

export function addCaptureToEndpoint(endpoint: Endpoint, capture: CaptureSummary): Endpoint {
  if (capture.seq <= endpoint.last_seq) {
    return endpoint;
  }
  return {
    ...endpoint,
    capture_count: Math.min(endpoint.max_captures, endpoint.capture_count + 1),
    last_seq: capture.seq,
  };
}

export function clearEndpointCaptures(endpoint: Endpoint): Endpoint {
  return { ...endpoint, capture_count: 0, bytes_used: 0 };
}

function updateCaptureLists(queryClient: QueryClient, endpointId: string, update: CaptureListUpdate): void {
  for (const query of queryClient.getQueryCache().findAll({ queryKey: queryKeys.captureLists(endpointId) })) {
    const data = queryClient.getQueryData<CaptureListData>(query.queryKey);
    if (data !== undefined) {
      queryClient.setQueryData(query.queryKey, update(data, readCaptureOrder(query.queryKey)));
    }
  }
}

function updateEndpointInCache(queryClient: QueryClient, endpointId: string, update: EndpointUpdate): void {
  queryClient.setQueryData<Endpoint>(queryKeys.endpoint(endpointId), (endpoint) =>
    endpoint === undefined ? undefined : update(endpoint),
  );
}

export function mergeCaptureIntoCache(queryClient: QueryClient, endpointId: string, capture: CaptureSummary): void {
  updateCaptureLists(queryClient, endpointId, (data, order) => mergeCaptureIntoPages(data, capture, order));
  updateEndpointInCache(queryClient, endpointId, (endpoint) => addCaptureToEndpoint(endpoint, capture));
}

export function removeCaptureFromCache(queryClient: QueryClient, endpointId: string, captureId: string): void {
  updateCaptureLists(queryClient, endpointId, (data) => removeCaptureFromPages(data, captureId));
  queryClient.removeQueries({ queryKey: queryKeys.capture(endpointId, captureId), type: 'inactive' });
}

export function clearCapturesInCache(queryClient: QueryClient, endpointId: string): void {
  updateCaptureLists(queryClient, endpointId, () => clearCapturePages());
  queryClient.removeQueries({ queryKey: queryKeys.captureDetails(endpointId), type: 'inactive' });
  updateEndpointInCache(queryClient, endpointId, clearEndpointCaptures);
}

function storeEndpoint(queryClient: QueryClient, endpoint: Endpoint): void {
  queryClient.setQueryData(queryKeys.endpoint(endpoint.id), endpoint);
  void queryClient.invalidateQueries({ queryKey: queryKeys.endpointLists() });
}

export function useInstance() {
  return useQuery({
    queryKey: queryKeys.instance(),
    queryFn: getInstance,
    retry: shouldRetryRequest,
  });
}

export function useEndpoint(endpointId: string) {
  return useQuery({
    queryKey: queryKeys.endpoint(endpointId),
    queryFn: () => getEndpoint(endpointId),
    retry: shouldRetryRequest,
  });
}

export function useCaptures(endpointId: string, filters: CaptureListFilters = {}) {
  return useInfiniteQuery<CapturePage, Error, CaptureListData, QueryKey, string | null>({
    queryKey: queryKeys.captureList(endpointId, filters),
    queryFn: ({ pageParam }) => listCaptures(endpointId, { ...filters, cursor: pageParam }),
    initialPageParam: FIRST_PAGE_CURSOR,
    getNextPageParam: (lastPage) => lastPage.next_cursor,
    retry: shouldRetryRequest,
  });
}

export function useCapture(endpointId: string, captureId: string) {
  return useQuery({
    queryKey: queryKeys.capture(endpointId, captureId),
    queryFn: () => getCapture(endpointId, captureId),
    retry: shouldRetryRequest,
  });
}

export function useCreateEndpoint() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: EndpointInput) => createEndpoint(input),
    onSuccess: (endpoint) => storeEndpoint(queryClient, endpoint),
  });
}

export function useUpdateEndpoint(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: EndpointInput) => patchEndpoint(endpointId, input),
    onSuccess: (endpoint) => storeEndpoint(queryClient, endpoint),
  });
}

export function useReplaceForwardRules(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (rules: ForwardRuleInput[]) => patchEndpoint(endpointId, { forward_rules: rules }),
    onSuccess: (endpoint) => storeEndpoint(queryClient, endpoint),
  });
}

export function useDeleteEndpoint(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => deleteEndpoint(endpointId),
    onSuccess: () => {
      queryClient.removeQueries({ queryKey: queryKeys.endpoint(endpointId) });
      queryClient.removeQueries({ queryKey: queryKeys.captures(endpointId) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.endpointLists() });
    },
  });
}

export function useDeleteCapture(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (captureId: string) => deleteCapture(endpointId, captureId),
    onSuccess: (_result, captureId) => {
      removeCaptureFromCache(queryClient, endpointId, captureId);
      void queryClient.invalidateQueries({ queryKey: queryKeys.endpoint(endpointId) });
    },
  });
}

export function useClearCaptures(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => clearCaptures(endpointId),
    onSuccess: () => clearCapturesInCache(queryClient, endpointId),
  });
}

export function useResendCapture(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (variables: ResendVariables) => resendCapture(endpointId, variables.captureId, variables.input),
    onSuccess: (_job, variables) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.capture(endpointId, variables.captureId) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.captureLists(endpointId) });
    },
  });
}
