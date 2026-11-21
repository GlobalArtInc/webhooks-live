import { useInfiniteQuery, useMutation, useQueryClient, type QueryClient, type QueryKey } from '@tanstack/react-query';
import { requestJson } from '../../../api/client';
import { queryKeys, shouldRetryRequest } from '../../../api/queries';
import type { Capture } from '../../../api/types';
import type { RuleRun, RuleRunFilter, RuleRunPage } from '../../../api/ruleRunTypes';
import { isRuleRunState } from './parse';
import { upsertRunInPages, withRuleRun, type RuleRunPages } from './model';

export interface RuleRunListQuery extends RuleRunFilter {
  cursor?: string | null;
  limit?: number;
}

export const ruleRunKeys = {
  all: (endpointId: string) => ['rule-runs', endpointId] as const,
  list: (endpointId: string, filter: RuleRunFilter) => ['rule-runs', endpointId, 'list', filter] as const,
};

function runsPath(endpointId: string): string {
  return `/endpoints/${encodeURIComponent(endpointId)}/rule-runs`;
}

function captureRunsPath(endpointId: string, captureId: string): string {
  return `/endpoints/${encodeURIComponent(endpointId)}/captures/${encodeURIComponent(captureId)}/rule-runs`;
}

export function listRuleRuns(endpointId: string, query: RuleRunListQuery): Promise<RuleRunPage> {
  return requestJson<RuleRunPage>(runsPath(endpointId), { method: 'GET', query: { ...query } });
}

export function rerunCapture(endpointId: string, captureId: string): Promise<RuleRun> {
  return requestJson<RuleRun>(captureRunsPath(endpointId, captureId), { method: 'POST' });
}

function filterOfKey(queryKey: QueryKey): RuleRunFilter {
  const filter: unknown = queryKey[3];
  if (typeof filter !== 'object' || filter === null || !('state' in filter)) {
    return {};
  }
  return isRuleRunState(filter.state) ? { state: filter.state } : {};
}

export function updateRuleRunLists(queryClient: QueryClient, endpointId: string, run: RuleRun): void {
  for (const [key, data] of queryClient.getQueriesData<RuleRunPages>({ queryKey: ruleRunKeys.all(endpointId) })) {
    if (data !== undefined) {
      queryClient.setQueryData<RuleRunPages>(key, upsertRunInPages(data, run, filterOfKey(key)));
    }
  }
}

export function storeRuleRun(queryClient: QueryClient, endpointId: string, run: RuleRun): void {
  queryClient.setQueryData<Capture>(queryKeys.capture(endpointId, run.capture_id), (current) =>
    current === undefined ? current : withRuleRun(current, run),
  );
  updateRuleRunLists(queryClient, endpointId, run);
}

export function useRuleRuns(endpointId: string, filter: RuleRunFilter = {}) {
  return useInfiniteQuery<RuleRunPage, Error, RuleRunPages, QueryKey, string | null>({
    queryKey: ruleRunKeys.list(endpointId, filter),
    queryFn: ({ pageParam }) => listRuleRuns(endpointId, { ...filter, cursor: pageParam }),
    initialPageParam: null,
    getNextPageParam: (lastPage) => lastPage.next_cursor,
    retry: shouldRetryRequest,
  });
}

export function useRerunCapture(endpointId: string, captureId: string) {
  const queryClient = useQueryClient();
  return useMutation<RuleRun, Error, void>({
    mutationFn: () => rerunCapture(endpointId, captureId),
    onSuccess: (run) => {
      storeRuleRun(queryClient, endpointId, run);
    },
  });
}
