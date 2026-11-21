import { useMatch, useNavigate } from '@tanstack/react-router';
import { useEffect, useMemo, useState } from 'react';
import { indexOfCapture, NOT_FOUND, resolveSeek, type CaptureListModel, type SeekTarget } from '../list';
import { rememberSelection } from './selectionMemory';

export const CAPTURE_ROUTE_ID = '/shell/e/$id/c/$captureId';

interface SeekRequest {
  target: SeekTarget;
  from: string | null;
}

export interface EndpointSelection {
  captureId: string | null;
  paneId: string | null;
  position: number | null;
  open: (captureId: string) => void;
  replace: (captureId: string | null) => void;
  first: () => void;
  previous: () => void;
  next: () => void;
  last: () => void;
}

export function useEndpointSelection(
  endpointId: string,
  list: CaptureListModel,
  implicitNewest: boolean,
): EndpointSelection {
  const navigate = useNavigate();
  const match = useMatch({ from: CAPTURE_ROUTE_ID, shouldThrow: false });
  const captureId = match?.params.captureId ?? null;
  const [request, setRequest] = useState<SeekRequest | null>(null);
  const { items, hasNextPage, isFetchingNextPage, error, fetchNextPage } = list;

  const activeSeek = useMemo<SeekTarget | null>(() => {
    if (request !== null && request.from === captureId) {
      return request.target;
    }
    return captureId === null ? null : { kind: 'load', captureId };
  }, [request, captureId]);

  useEffect(() => {
    if (captureId !== null) {
      rememberSelection(endpointId, captureId);
    }
  }, [captureId, endpointId]);

  useEffect(() => {
    if (activeSeek === null) {
      return;
    }
    const outcome = resolveSeek(items, hasNextPage, activeSeek);
    if (outcome.status === 'pending') {
      if (!isFetchingNextPage && error === null) {
        fetchNextPage();
      }
      return;
    }
    if (outcome.captureId !== null && activeSeek.kind !== 'load') {
      void navigate({ to: '/e/$id/c/$captureId', params: { id: endpointId, captureId: outcome.captureId } });
    }
  }, [activeSeek, items, hasNextPage, isFetchingNextPage, error, fetchNextPage, endpointId, navigate]);

  const paneId = captureId ?? (implicitNewest ? (items[0]?.id ?? null) : null);
  const index = paneId === null ? NOT_FOUND : indexOfCapture(items, paneId);

  function open(id: string): void {
    void navigate({ to: '/e/$id/c/$captureId', params: { id: endpointId, captureId: id } });
  }

  function replace(id: string | null): void {
    if (id === null) {
      void navigate({ to: '/e/$id', params: { id: endpointId }, replace: true });
      return;
    }
    void navigate({ to: '/e/$id/c/$captureId', params: { id: endpointId, captureId: id }, replace: true });
  }

  function requestSeek(target: SeekTarget): void {
    setRequest({ target, from: captureId });
  }

  function first(): void {
    const newest = items[0];
    if (newest !== undefined) {
      open(newest.id);
    }
  }

  function previous(): void {
    const newer = index > 0 ? items[index - 1] : undefined;
    if (newer !== undefined) {
      open(newer.id);
    }
  }

  function next(): void {
    if (paneId === null || index === NOT_FOUND) {
      return;
    }
    const older = items[index + 1];
    if (older !== undefined) {
      open(older.id);
      return;
    }
    requestSeek({ kind: 'next', captureId: paneId });
  }

  function last(): void {
    requestSeek({ kind: 'last' });
  }

  return {
    captureId,
    paneId,
    position: index === NOT_FOUND ? null : index + 1,
    open,
    replace,
    first,
    previous,
    next,
    last,
  };
}
