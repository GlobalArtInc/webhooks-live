import { useQueries, useQuery } from '@tanstack/react-query';
import { getEndpoint, listFolders } from '../../api/client';
import { queryKeys, shouldRetryRequest } from '../../api/queries';
import type { Endpoint, EndpointRole, Folder } from '../../api/types';

const FOLDERS_KEY = ['folders'] as const;
const WRITE_ROLES: readonly (EndpointRole | null)[] = ['owner', 'editor'];

export function canDeleteWith(endpoint: Endpoint | undefined): boolean {
  return endpoint !== undefined && WRITE_ROLES.includes(endpoint.my_role);
}

export function useInboxEndpoints(endpointIds: readonly string[]): ReadonlyMap<string, Endpoint> {
  const results = useQueries({
    queries: endpointIds.map((endpointId) => ({
      queryKey: queryKeys.endpoint(endpointId),
      queryFn: () => getEndpoint(endpointId),
      retry: shouldRetryRequest,
    })),
  });
  const known = new Map<string, Endpoint>();
  results.forEach((result, index) => {
    const endpointId = endpointIds[index];
    if (endpointId !== undefined && result.data !== undefined) {
      known.set(endpointId, result.data);
    }
  });
  return known;
}

export function useFolders(enabled: boolean): readonly Folder[] {
  const query = useQuery({
    queryKey: FOLDERS_KEY,
    queryFn: listFolders,
    enabled,
    retry: shouldRetryRequest,
  });
  return query.data?.items ?? [];
}
