import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { endpointPath, requestEmpty, requestJson } from './client';
import { queryKeys, shouldRetryRequest } from './queries';
import type { Endpoint, ItemList, Timestamp } from './types';

export const CLONE_PARTS = [
  'settings',
  'response',
  'forwarding',
  'rules',
  'schedules',
  'channels',
  'verifier',
] as const;

export type ClonePart = (typeof CLONE_PARTS)[number];

export interface Alias {
  alias: string;
  url: string;
  created_at: Timestamp;
}

export type AliasList = ItemList<Alias>;

export type AliasReason = 'invalid' | 'reserved' | 'taken';

export interface AliasAvailability {
  alias: string;
  available: boolean;
  reason: AliasReason | null;
}

export interface UsageMeter {
  name: string;
  used: number;
  max: number | null;
  window: string | null;
  resets_at: Timestamp | null;
}

export interface UsageReport {
  meters: UsageMeter[];
}

export interface CloneInput {
  title: string;
  parts: ClonePart[];
}

export interface CloneResult extends Endpoint {
  secrets_missing: string[];
}

export const generalQueryKeys = {
  aliases: (endpointId: string) => ['endpoint', endpointId, 'aliases'] as const,
  usage: () => ['usage'] as const,
};

function aliasPath(endpointId: string, alias: string): string {
  return `${endpointPath(endpointId)}/aliases/${encodeURIComponent(alias)}`;
}

export function listAliases(endpointId: string): Promise<AliasList> {
  return requestJson<AliasList>(`${endpointPath(endpointId)}/aliases`, { method: 'GET' });
}

export function putAlias(endpointId: string, alias: string): Promise<Alias> {
  return requestJson<Alias>(aliasPath(endpointId, alias), { method: 'PUT' });
}

export function deleteAlias(endpointId: string, alias: string): Promise<void> {
  return requestEmpty(aliasPath(endpointId, alias), { method: 'DELETE' });
}

export function checkAliasAvailability(alias: string, signal?: AbortSignal): Promise<AliasAvailability> {
  return requestJson<AliasAvailability>(`/aliases/${encodeURIComponent(alias)}/availability`, {
    method: 'GET',
    signal,
  });
}

export function getUsage(): Promise<UsageReport> {
  return requestJson<UsageReport>('/usage', { method: 'GET' });
}

export function cloneEndpoint(endpointId: string, input: CloneInput): Promise<CloneResult> {
  return requestJson<CloneResult>(`${endpointPath(endpointId)}/clone`, { method: 'POST', body: input });
}

function invalidateAliases(queryClient: QueryClient, endpointId: string): void {
  void queryClient.invalidateQueries({ queryKey: generalQueryKeys.aliases(endpointId) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.endpoint(endpointId) });
}

export function useAliases(endpointId: string) {
  return useQuery({
    queryKey: generalQueryKeys.aliases(endpointId),
    queryFn: () => listAliases(endpointId),
    retry: shouldRetryRequest,
  });
}

export function useAddAlias(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (alias: string) => putAlias(endpointId, alias),
    onSuccess: () => {
      invalidateAliases(queryClient, endpointId);
    },
  });
}

export function useRemoveAlias(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (alias: string) => deleteAlias(endpointId, alias),
    onSuccess: () => {
      invalidateAliases(queryClient, endpointId);
    },
  });
}

export function useUsage() {
  return useQuery({
    queryKey: generalQueryKeys.usage(),
    queryFn: getUsage,
    retry: shouldRetryRequest,
  });
}

export function useCloneEndpoint(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CloneInput) => cloneEndpoint(endpointId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.endpointLists() });
    },
  });
}
