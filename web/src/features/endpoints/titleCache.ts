import { useQueries, useQuery } from '@tanstack/react-query';
import { getEndpoint } from '../../api/client';
import { queryKeys } from '../../api/queries';
import type { Endpoint } from '../../api/types';

function titleOf(endpoint: Endpoint): string | null {
  const title = endpoint.title?.trim();
  return title === undefined || title === '' ? null : title;
}

export function useCachedEndpointTitle(endpointId: string | null): string | null {
  const id = endpointId ?? '';
  const query = useQuery({
    queryKey: queryKeys.endpoint(id),
    queryFn: () => getEndpoint(id),
    enabled: false,
    select: titleOf,
  });
  return endpointId === null ? null : (query.data ?? null);
}

export function useCachedEndpointTitles(endpointIds: readonly string[]): readonly (string | null)[] {
  return useQueries({
    queries: endpointIds.map((id) => ({
      queryKey: queryKeys.endpoint(id),
      queryFn: () => getEndpoint(id),
      enabled: false,
      select: titleOf,
    })),
    combine: (results) => results.map((result) => result.data ?? null),
  });
}
