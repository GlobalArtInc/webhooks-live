import { useMutation, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { createForwardRule, deleteForwardRule, getLatestCapture, patchForwardRule, resendCapture } from './client';
import { queryKeys } from './queries';
import type { ForwardJob, ForwardRuleInput } from './types';

export interface RulePatch {
  ruleId: string;
  input: Partial<ForwardRuleInput>;
}

export interface RuleTestResult {
  captureId: string;
  job: ForwardJob;
}

function refreshEndpoint(queryClient: QueryClient, endpointId: string): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.endpoint(endpointId) });
}

export function useCreateForwardRule(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: ForwardRuleInput) => createForwardRule(endpointId, input),
    onSuccess: () => refreshEndpoint(queryClient, endpointId),
  });
}

export function usePatchForwardRule(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ ruleId, input }: RulePatch) => patchForwardRule(endpointId, ruleId, input),
    onSuccess: () => refreshEndpoint(queryClient, endpointId),
  });
}

export function useDeleteForwardRule(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (ruleId: string) => deleteForwardRule(endpointId, ruleId),
    onSuccess: () => refreshEndpoint(queryClient, endpointId),
  });
}

export function useTestForwardRule(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (ruleId: string): Promise<RuleTestResult> => {
      const capture = await getLatestCapture(endpointId);
      const job = await resendCapture(endpointId, capture.id, { rule_id: ruleId });
      return { captureId: capture.id, job };
    },
    onSuccess: ({ captureId }) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.capture(endpointId, captureId) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.captureLists(endpointId) });
    },
  });
}
