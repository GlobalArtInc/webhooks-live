import { useQuery, type QueryClient } from '@tanstack/react-query';
import { requestJson } from './client';
import { isLimitFailure } from './quota';
import { shouldRetryRequest } from './queries';
import type { Timestamp } from './types';

export type UsageWindow = 'total' | 'hour' | 'day';

export interface UsageMeter {
  name: string;
  used: number;
  max: number;
  window: UsageWindow;
  resets_at: Timestamp | null;
}

export interface Usage {
  tier: string;
  items: UsageMeter[];
}

export const usageQueryKeys = {
  usage: () => ['usage'] as const,
};

export function getUsage(): Promise<Usage> {
  return requestJson<Usage>('/usage', { method: 'GET' });
}

export function useUsage(enabled: boolean) {
  return useQuery({
    queryKey: usageQueryKeys.usage(),
    queryFn: getUsage,
    retry: shouldRetryRequest,
    refetchOnWindowFocus: true,
    enabled,
  });
}

export function refreshUsageOnLimit(queryClient: QueryClient, error: unknown): void {
  if (isLimitFailure(error)) {
    void queryClient.invalidateQueries({ queryKey: usageQueryKeys.usage() });
  }
}
