import { useQuery } from '@tanstack/react-query';
import { httpScriptTransport } from './transport';
import type { ScriptLanguageMeta, ScriptLanguageState, ScriptTransport } from './types';

const META_STALE_MS = 10 * 60 * 1000;
const META_RETRIES = 1;
const META_RETRY_DELAY_MS = 500;

export interface ScriptMetaResult {
  meta: ScriptLanguageMeta | null;
  state: ScriptLanguageState;
  retry: () => void;
}

export function scriptMetaKey(transport: ScriptTransport): readonly string[] {
  return ['script', 'meta', transport.id];
}

export function useScriptMeta(transport: ScriptTransport = httpScriptTransport): ScriptMetaResult {
  const query = useQuery({
    queryKey: scriptMetaKey(transport),
    queryFn: ({ signal }) => transport.loadMeta(signal),
    staleTime: META_STALE_MS,
    retry: META_RETRIES,
    retryDelay: META_RETRY_DELAY_MS,
  });
  let state: ScriptLanguageState = 'loading';
  if (query.data !== undefined) {
    state = 'ready';
  } else if (query.isError) {
    state = 'failed';
  }
  return {
    meta: query.data ?? null,
    state,
    retry: () => {
      void query.refetch();
    },
  };
}
