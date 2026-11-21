import type { QueryClient } from '@tanstack/react-query';
import { getEndpoint } from '../api/client';
import { queryKeys } from '../api/queries';
import type { Endpoint } from '../api/types';

export function ensureEndpoint(queryClient: QueryClient, endpointId: string): Promise<Endpoint> {
  return queryClient.ensureQueryData({
    queryKey: queryKeys.endpoint(endpointId),
    queryFn: () => getEndpoint(endpointId),
  });
}
