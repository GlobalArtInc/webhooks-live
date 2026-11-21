import { useSyncExternalStore } from 'react';
import { NARROW_VIEWPORT_QUERY } from './constants';

function subscribe(onChange: () => void): () => void {
  if (typeof window.matchMedia !== 'function') {
    return () => undefined;
  }
  const query = window.matchMedia(NARROW_VIEWPORT_QUERY);
  query.addEventListener('change', onChange);
  return () => {
    query.removeEventListener('change', onChange);
  };
}

function readNarrow(): boolean {
  return typeof window.matchMedia === 'function' && window.matchMedia(NARROW_VIEWPORT_QUERY).matches;
}

export function useNarrowViewport(): boolean {
  return useSyncExternalStore(subscribe, readNarrow, () => false);
}
