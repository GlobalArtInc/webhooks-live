import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useDeleteCapture } from '../../api/queries';
import type { CaptureSummary } from '../../api/types';
import { hideCapture, showCapture } from '../list/cache';
import { UNDO_WINDOW_MS } from '../list/useDeleteWithUndo';

export interface BulkDelete {
  pending: CaptureSummary[] | null;
  failed: boolean;
  schedule: (captures: CaptureSummary[]) => void;
  undo: () => void;
  commit: () => void;
}

export function useBulkDelete(endpointId: string): BulkDelete {
  const queryClient = useQueryClient();
  const { mutateAsync } = useDeleteCapture(endpointId);
  const [pending, setPending] = useState<CaptureSummary[] | null>(null);
  const [failed, setFailed] = useState(false);
  const pendingRef = useRef<CaptureSummary[] | null>(null);
  const timerRef = useRef<number | undefined>(undefined);

  const commit = useCallback(() => {
    window.clearTimeout(timerRef.current);
    const current = pendingRef.current;
    if (current === null) {
      return;
    }
    pendingRef.current = null;
    setPending(null);
    void deleteEach(current, (capture) => mutateAsync(capture.id)).then((restored) => {
      for (const capture of restored) {
        showCapture(queryClient, endpointId, capture);
      }
      if (restored.length > 0) {
        setFailed(true);
      }
    });
  }, [endpointId, mutateAsync, queryClient]);

  const schedule = useCallback(
    (captures: CaptureSummary[]) => {
      commit();
      if (captures.length === 0) {
        return;
      }
      for (const capture of captures) {
        hideCapture(queryClient, endpointId, capture.id);
      }
      pendingRef.current = captures;
      setPending(captures);
      setFailed(false);
      timerRef.current = window.setTimeout(commit, UNDO_WINDOW_MS);
    },
    [commit, endpointId, queryClient],
  );

  const undo = useCallback(() => {
    window.clearTimeout(timerRef.current);
    const current = pendingRef.current;
    if (current === null) {
      return;
    }
    pendingRef.current = null;
    setPending(null);
    for (const capture of current) {
      showCapture(queryClient, endpointId, capture);
    }
  }, [endpointId, queryClient]);

  useEffect(() => () => commit(), [commit]);

  return { pending, failed, schedule, undo, commit };
}

async function deleteEach(
  captures: CaptureSummary[],
  remove: (capture: CaptureSummary) => Promise<void>,
): Promise<CaptureSummary[]> {
  const restored: CaptureSummary[] = [];
  for (const capture of captures) {
    try {
      await remove(capture);
    } catch {
      restored.push(capture);
    }
  }
  return restored;
}
