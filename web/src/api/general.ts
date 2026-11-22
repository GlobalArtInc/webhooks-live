import { useMutation, useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { endpointPath, requestEmpty, requestJson } from './client';
import { queryKeys, shouldRetryRequest } from './queries';
import type { Endpoint, ItemList, Timestamp } from './types';

export const CLONE_PARTS = ['response', 'forward_rules', 'rules', 'channels', 'schedules'] as const;

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

export interface CloneInput {
  title?: string;
  include?: ClonePart[];
}

export interface CloneResult extends Endpoint {
  secrets_missing: string[];
  skipped: string[];
}

export const generalQueryKeys = {
  aliases: (endpointId: string) => ['endpoint', endpointId, 'aliases'] as const,
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

export function cloneEndpoint(endpointId: string, input: CloneInput): Promise<CloneResult> {
  return requestJson<CloneResult>(`${endpointPath(endpointId)}/clone`, { method: 'POST', body: input });
}

function invalidateAliases(queryClient: QueryClient, endpointId: string): void {
  void queryClient.invalidateQueries({ queryKey: generalQueryKeys.aliases(endpointId) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.endpoint(endpointId) });
}

export function useAliases(endpointId: string, enabled: boolean) {
  return useQuery({
    queryKey: generalQueryKeys.aliases(endpointId),
    queryFn: () => listAliases(endpointId),
    retry: shouldRetryRequest,
    enabled,
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

export function useCloneEndpoint(endpointId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CloneInput) => cloneEndpoint(endpointId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.endpointLists() });
    },
  });
}
