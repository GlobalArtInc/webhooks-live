import { useNavigate, useSearch } from '@tanstack/react-router';
import { useCallback } from 'react';
import type { CaptureSearch, CaptureSearchPatch } from './captureSearch';

export const ENDPOINT_SEARCH_ROUTE_ID = '/shell/e/$id';
const ENDPOINT_SEARCH_PATH = '/e/$id';

export interface CaptureSearchControls {
  search: CaptureSearch;
  update: (patch: CaptureSearchPatch) => void;
  clear: () => void;
}

export function useCaptureSearch(): CaptureSearchControls {
  const search = useSearch({ from: ENDPOINT_SEARCH_ROUTE_ID });
  const navigate = useNavigate({ from: ENDPOINT_SEARCH_PATH });
  const update = useCallback(
    (patch: CaptureSearchPatch) => {
      void navigate({ search: (previous) => ({ ...previous, ...patch }), replace: true });
    },
    [navigate],
  );
  const clear = useCallback(() => {
    void navigate({ search: {}, replace: true });
  }, [navigate]);
  return { search, update, clear };
}
