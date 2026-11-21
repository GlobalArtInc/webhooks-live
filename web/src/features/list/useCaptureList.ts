import { useMemo } from 'react';
import { useCaptures } from '../../api/queries';
import type { CaptureListFilters, CaptureSummary } from '../../api/types';
import { flattenCaptures, totalOf } from './listModel';

export const CAPTURE_LIST_FILTERS: CaptureListFilters = { order: 'desc', limit: 100 };

export interface CaptureListModel {
  items: CaptureSummary[];
  total: number | null;
  isPending: boolean;
  error: Error | null;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  hasData: boolean;
  fetchNextPage: () => void;
  retry: () => void;
}

export function useCaptureList(endpointId: string): CaptureListModel {
  const query = useCaptures(endpointId, CAPTURE_LIST_FILTERS);
  const items = useMemo(() => flattenCaptures(query.data), [query.data]);

  return {
    items,
    total: totalOf(query.data),
    isPending: query.isPending,
    error: query.error,
    hasNextPage: query.hasNextPage,
    isFetchingNextPage: query.isFetchingNextPage,
    hasData: query.data !== undefined,
    fetchNextPage: () => {
      void query.fetchNextPage({ cancelRefetch: false });
    },
    retry: () => {
      void query.refetch();
    },
  };
}
