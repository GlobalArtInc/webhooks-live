import { useSyncExternalStore } from 'react';

export interface DiffRouteParams {
  id: string;
  a: string;
  b: string;
}

type MarkedCaptures = ReadonlyMap<string, string>;

let marked: MarkedCaptures = new Map();
const listeners = new Set<() => void>();

function publish(next: MarkedCaptures): void {
  marked = next;
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function readMarked(endpointId: string): string | null {
  return marked.get(endpointId) ?? null;
}

export function getMarkedCapture(endpointId: string): string | null {
  return readMarked(endpointId);
}

export function markCapture(endpointId: string, captureId: string): void {
  publish(new Map(marked).set(endpointId, captureId));
}

export function clearMarkedCapture(endpointId: string): void {
  if (!marked.has(endpointId)) {
    return;
  }
  const next = new Map(marked);
  next.delete(endpointId);
  publish(next);
}

export function compareWith(endpointId: string, captureId: string): DiffRouteParams | null {
  const markedId = readMarked(endpointId);
  return markedId === null ? null : { id: endpointId, a: markedId, b: captureId };
}

export function useMarkedCapture(endpointId: string): string | null {
  return useSyncExternalStore(
    subscribe,
    () => readMarked(endpointId),
    () => null,
  );
}
