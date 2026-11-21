import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { requestEmpty, requestJson } from './client';
import { queryKeys, shouldRetryRequest } from './queries';
import type { Rule, RuleInput, RuleList, RuleOrderInput } from './rulesTypes';

export interface RuleEnabledChange {
  ruleId: string;
  enabled: boolean;
}

export interface RuleListContext {
  previous: RuleList | undefined;
}

function rulesPath(endpointId: string): string {
  return `/endpoints/${encodeURIComponent(endpointId)}/rules`;
}

function rulePath(endpointId: string, ruleId: string): string {
  return `${rulesPath(endpointId)}/${encodeURIComponent(ruleId)}`;
}

export function rulesQueryKey(endpointId: string) {
  return [...queryKeys.endpoint(endpointId), 'rules'] as const;
}

export function listRules(endpointId: string): Promise<RuleList> {
  return requestJson<RuleList>(rulesPath(endpointId), { method: 'GET' });
}

export function patchRule(endpointId: string, ruleId: string, input: RuleInput): Promise<Rule> {
  return requestJson<Rule>(rulePath(endpointId, ruleId), { method: 'PATCH', body: input });
}

export function deleteRule(endpointId: string, ruleId: string): Promise<void> {
  return requestEmpty(rulePath(endpointId, ruleId), { method: 'DELETE' });
}

export function reorderRules(endpointId: string, ids: string[]): Promise<RuleList> {
  const body: RuleOrderInput = { ids };
  return requestJson<RuleList>(`${rulesPath(endpointId)}/order`, { method: 'PUT', body });
}

export function withOrder(list: RuleList, ids: readonly string[]): RuleList {
  const byId = new Map<string, Rule>(list.items.map((rule) => [rule.id, rule] as const));
  const items = ids.flatMap((id) => {
    const rule = byId.get(id);
    return rule === undefined ? [] : [rule];
  });
  return { ...list, items };
}

export function withRule(list: RuleList, rule: Rule): RuleList {
  return { ...list, items: list.items.map((item) => (item.id === rule.id ? rule : item)) };
}

export function withEnabled(list: RuleList, ruleId: string, enabled: boolean): RuleList {
  return { ...list, items: list.items.map((item) => (item.id === ruleId ? { ...item, enabled } : item)) };
}

export function withoutRule(list: RuleList, ruleId: string): RuleList {
  return { ...list, items: list.items.filter((item) => item.id !== ruleId) };
}

function restoreList(queryClient: QueryClient, key: readonly unknown[], previous: RuleList | undefined): void {
  if (previous !== undefined) {
    queryClient.setQueryData<RuleList>(key, previous);
  }
}

function refreshList(queryClient: QueryClient, key: readonly unknown[]): void {
  void queryClient.invalidateQueries({ queryKey: key });
}

export function useRules(endpointId: string) {
  return useQuery({
    queryKey: rulesQueryKey(endpointId),
    queryFn: () => listRules(endpointId),
    retry: shouldRetryRequest,
  });
}

export function useSetRuleEnabled(endpointId: string) {
  const queryClient = useQueryClient();
  const key = rulesQueryKey(endpointId);
  return useMutation<Rule, Error, RuleEnabledChange, RuleListContext>({
    mutationFn: ({ ruleId, enabled }) => patchRule(endpointId, ruleId, { enabled }),
    onMutate: async ({ ruleId, enabled }) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<RuleList>(key);
      if (previous !== undefined) {
        queryClient.setQueryData<RuleList>(key, withEnabled(previous, ruleId, enabled));
      }
      return { previous };
    },
    onError: (_error, _change, context) => {
      restoreList(queryClient, key, context?.previous);
    },
    onSuccess: (rule) => {
      const current = queryClient.getQueryData<RuleList>(key);
      if (current !== undefined) {
        queryClient.setQueryData<RuleList>(key, withRule(current, rule));
      }
    },
    onSettled: () => {
      refreshList(queryClient, key);
    },
  });
}

export function useDeleteRule(endpointId: string) {
  const queryClient = useQueryClient();
  const key = rulesQueryKey(endpointId);
  return useMutation<void, Error, string>({
    mutationFn: (ruleId) => deleteRule(endpointId, ruleId),
    onSuccess: (_result, ruleId) => {
      const current = queryClient.getQueryData<RuleList>(key);
      if (current !== undefined) {
        queryClient.setQueryData<RuleList>(key, withoutRule(current, ruleId));
      }
      refreshList(queryClient, key);
    },
  });
}

export function useReorderRules(endpointId: string) {
  const queryClient = useQueryClient();
  const key = rulesQueryKey(endpointId);
  return useMutation<RuleList, Error, string[], RuleListContext>({
    mutationFn: (ids) => reorderRules(endpointId, ids),
    onMutate: async (ids) => {
      await queryClient.cancelQueries({ queryKey: key });
      const previous = queryClient.getQueryData<RuleList>(key);
      if (previous !== undefined) {
        queryClient.setQueryData<RuleList>(key, withOrder(previous, ids));
      }
      return { previous };
    },
    onError: (_error, _ids, context) => {
      restoreList(queryClient, key, context?.previous);
    },
    onSuccess: (list) => {
      queryClient.setQueryData<RuleList>(key, list);
    },
  });
}
