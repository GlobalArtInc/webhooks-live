import { useNavigate, useParams, useSearch } from '@tanstack/react-router';
import { useEffect, useRef, useState } from 'react';
import { ApiReferencePage } from '../../features/apireference';

const QUERY_SYNC_MS = 250;

export function ApiReferenceScreen() {
  const { operationId } = useParams({ strict: false });
  const search = useSearch({ from: '/shell/docs/api' });
  const navigate = useNavigate();
  const [query, setQuery] = useState(search.q ?? '');
  const synced = useRef(search.q ?? '');

  useEffect(() => {
    const next = query.trim();
    if (next === synced.current) {
      return undefined;
    }
    const timer = window.setTimeout(() => {
      synced.current = next;
      const nextSearch = next === '' ? {} : { q: next };
      if (operationId === undefined) {
        void navigate({ to: '/docs/api', search: nextSearch, replace: true });
      } else {
        void navigate({ to: '/docs/api/$operationId', params: { operationId }, search: nextSearch, replace: true });
      }
    }, QUERY_SYNC_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [query, navigate, operationId]);

  return <ApiReferencePage operationId={operationId ?? null} query={query} onQueryChange={setQuery} />;
}
