import { useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useDeleteCapture } from '../../api/queries';
import type { CaptureSummary } from '../../api/types';
import { hideCapture, showCapture } from './cache';

export const UNDO_WINDOW_MS = 5000;

export interface PendingDeletion {
  capture: CaptureSummary;
  wasSelected: boolean;
}

export interface DeleteWithUndo {
  pending: PendingDeletion | null;
  failed: boolean;
  schedule: (capture: CaptureSummary, wasSelected: boolean) => void;
  undo: () => PendingDeletion | null;
  commit: () => void;
}

export function useDeleteWithUndo(endpointId: string): DeleteWithUndo {
  const queryClient = useQueryClient();
  const { mutateAsync } = useDeleteCapture(endpointId);
  const [pending, setPending] = useState<PendingDeletion | null>(null);
  const [failed, setFailed] = useState(false);
  const pendingRef = useRef<PendingDeletion | null>(null);
  const timerRef = useRef<number | undefined>(undefined);

  const commit = useCallback(() => {
    window.clearTimeout(timerRef.current);
    const current = pendingRef.current;
    if (current === null) {
      return;
    }
    pendingRef.current = null;
    setPending(null);
    mutateAsync(current.capture.id).catch(() => {
      showCapture(queryClient, endpointId, current.capture);
      setFailed(true);
    });
  }, [endpointId, mutateAsync, queryClient]);

  const schedule = useCallback(
    (capture: CaptureSummary, wasSelected: boolean) => {
      commit();
      hideCapture(queryClient, endpointId, capture.id);
      const next: PendingDeletion = { capture, wasSelected };
      pendingRef.current = next;
      setPending(next);
      setFailed(false);
      timerRef.current = window.setTimeout(commit, UNDO_WINDOW_MS);
    },
    [commit, endpointId, queryClient],
  );

  const undo = useCallback((): PendingDeletion | null => {
    window.clearTimeout(timerRef.current);
    const current = pendingRef.current;
    if (current === null) {
      return null;
    }
    pendingRef.current = null;
    setPending(null);
    showCapture(queryClient, endpointId, current.capture);
    return current;
  }, [endpointId, queryClient]);

  useEffect(() => () => commit(), [commit]);

  return { pending, failed, schedule, undo, commit };
}
