import { useQueryClient } from '@tanstack/react-query';
import { useEffect, useRef, useSyncExternalStore } from 'react';
import { activeFiltersKey, NO_FILTERS, type ActiveFilters } from '../list/listFilters';
import { highestHeldSeq } from './catchUp';
import { LiveSource } from './LiveSource';
import { LiveSession } from './session';
import { getLiveStore, type LiveSnapshot } from './store';

export interface LiveEndpointOptions {
  following: boolean;
  selectedId: string | null;
  onFollowNewest: (captureId: string) => void;
  filters?: ActiveFilters;
}

export function useLiveEndpoint(endpointId: string, options: LiveEndpointOptions): LiveSnapshot {
  const queryClient = useQueryClient();
  const store = getLiveStore(endpointId);
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  const optionsRef = useRef(options);
  const { selectedId } = options;
  const filterKey = activeFiltersKey(options.filters ?? NO_FILTERS);

  useEffect(() => {
    optionsRef.current = options;
  });

  useEffect(() => {
    store.clearHidden();
  }, [store, filterKey]);

  useEffect(() => {
    const source = new LiveSource({
      endpointId,
      getLastSeq: () => highestHeldSeq(queryClient, endpointId),
    });
    const session = new LiveSession({
      queryClient,
      endpointId,
      source,
      store,
      getContext: () => ({ hidden: document.hidden, following: optionsRef.current.following }),
      getFilters: () => optionsRef.current.filters ?? NO_FILTERS,
      onFollow: (captureId) => {
        optionsRef.current.onFollowNewest(captureId);
      },
    });
    session.start();
    return () => {
      session.dispose();
    };
  }, [endpointId, queryClient, store]);

  useEffect(() => {
    if (selectedId !== null) {
      store.markRead(selectedId);
    }
  }, [store, selectedId]);

  useEffect(() => {
    function clearAway(): void {
      if (!document.hidden) {
        store.clearAway();
      }
    }
    document.addEventListener('visibilitychange', clearAway);
    window.addEventListener('focus', clearAway);
    return () => {
      document.removeEventListener('visibilitychange', clearAway);
      window.removeEventListener('focus', clearAway);
    };
  }, [store]);

  return snapshot;
}
