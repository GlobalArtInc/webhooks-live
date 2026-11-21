import { useInfiniteQuery, useMutation, useQuery, useQueryClient, type InfiniteData } from '@tanstack/react-query';
import { listEndpoints, requestEmpty, requestJson } from './client';
import { shouldRetryRequest } from './queries';
import type { Endpoint, EndpointPage, ItemList, Timestamp, Uuid } from './types';

export const TOKEN_SCOPES = ['read', 'endpoints:write', 'captures:write', 'rules:write', 'send'] as const;

export type TokenScope = (typeof TOKEN_SCOPES)[number];

export interface ApiToken {
  id: Uuid;
  name: string;
  prefix: string;
  scopes: string[];
  endpoint_ids: string[] | null;
  created_at: Timestamp;
  expires_at: Timestamp;
  last_used_at: Timestamp | null;
  revoked_at: Timestamp | null;
}

export interface CreatedApiToken extends ApiToken {
  token: string;
}

export interface ApiTokenInput {
  name: string;
  scopes: TokenScope[];
  endpoint_ids?: string[];
  expires_in_days: number;
}

export type ApiTokenList = ItemList<ApiToken>;

export interface CreateTokenVariables {
  input: ApiTokenInput;
  cancel: AbortSignal;
}

type OwnedEndpointData = InfiniteData<EndpointPage, string | null>;

const OWNED_PAGE_SIZE = 100;
const CREATE_TIMEOUT_MS = 30_000;
const FIRST_PAGE_CURSOR: string | null = null;

export const tokenQueryKeys = {
  list: () => ['api-tokens'] as const,
  ownedEndpoints: () => ['endpoint-list', 'owned-for-tokens'] as const,
};

export function listApiTokens(): Promise<ApiTokenList> {
  return requestJson<ApiTokenList>('/tokens', { method: 'GET' });
}

export function createApiToken(input: ApiTokenInput, cancel?: AbortSignal): Promise<CreatedApiToken> {
  const timeout = AbortSignal.timeout(CREATE_TIMEOUT_MS);
  const signal = cancel === undefined ? timeout : AbortSignal.any([timeout, cancel]);
  return requestJson<CreatedApiToken>('/tokens', { method: 'POST', body: input, signal });
}

export function revokeApiToken(tokenId: string): Promise<void> {
  return requestEmpty(`/tokens/${encodeURIComponent(tokenId)}`, { method: 'DELETE' });
}

export function useApiTokens() {
  return useQuery({
    queryKey: tokenQueryKeys.list(),
    queryFn: listApiTokens,
    retry: shouldRetryRequest,
  });
}

export function useCreateApiToken() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ input, cancel }: CreateTokenVariables) => createApiToken(input, cancel),
    gcTime: 0,
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: tokenQueryKeys.list() });
    },
  });
}

export function useRevokeApiToken() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: revokeApiToken,
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: tokenQueryKeys.list() });
    },
  });
}

export function useOwnedEndpoints(enabled: boolean) {
  return useInfiniteQuery<EndpointPage, Error, OwnedEndpointData, readonly string[], string | null>({
    queryKey: tokenQueryKeys.ownedEndpoints(),
    queryFn: ({ pageParam }) => listEndpoints({ role: 'owner', limit: OWNED_PAGE_SIZE, cursor: pageParam }),
    initialPageParam: FIRST_PAGE_CURSOR,
    getNextPageParam: (lastPage) => lastPage.next_cursor,
    retry: shouldRetryRequest,
    enabled,
  });
}

export function flattenOwnedEndpoints(data: OwnedEndpointData | undefined): Endpoint[] {
  return data === undefined ? [] : data.pages.flatMap((page) => page.items);
}
