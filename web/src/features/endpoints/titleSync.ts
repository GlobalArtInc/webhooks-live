import type { QueryClient } from '@tanstack/react-query';
import { recordEndpointTitle } from './store';

const ENDPOINT_KEY_HEAD = 'endpoint';
const ENDPOINT_KEY_LENGTH = 2;

interface TitledEndpoint {
  id: string;
  title: string | null;
}

function isTitledEndpoint(value: unknown): value is TitledEndpoint {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const id: unknown = Reflect.get(value, 'id');
  const title: unknown = Reflect.get(value, 'title');
  return typeof id === 'string' && (title === null || typeof title === 'string');
}

export function syncEndpointTitles(queryClient: QueryClient): () => void {
  return queryClient.getQueryCache().subscribe((event) => {
    if (event.type !== 'updated' || event.action.type !== 'success') {
      return;
    }
    const { queryKey, state } = event.query;
    if (queryKey[0] !== ENDPOINT_KEY_HEAD || queryKey.length !== ENDPOINT_KEY_LENGTH) {
      return;
    }
    if (isTitledEndpoint(state.data)) {
      recordEndpointTitle(state.data.id, state.data.title);
    }
  });
}
