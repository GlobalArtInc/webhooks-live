export interface ApiReferenceSearch {
  q?: string;
}

export const MAX_QUERY_LENGTH = 200;

export function validateApiReferenceSearch(search: Record<string, unknown>): ApiReferenceSearch {
  if (typeof search.q !== 'string') {
    return {};
  }
  const q = search.q.trim().slice(0, MAX_QUERY_LENGTH);
  return q === '' ? {} : { q };
}
