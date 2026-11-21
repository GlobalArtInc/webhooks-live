import { useCallback, useSyncExternalStore } from 'react';
import { NARROW_QUERY, WIDE_QUERY } from './constants';

export type ViewportLayout = 'narrow' | 'medium' | 'wide';

function subscribeTo(query: string, onChange: () => void): () => void {
  if (typeof window.matchMedia !== 'function') {
    return () => undefined;
  }
  const list = window.matchMedia(query);
  list.addEventListener('change', onChange);
  return () => {
    list.removeEventListener('change', onChange);
  };
}

function matches(query: string): boolean {
  return typeof window.matchMedia === 'function' && window.matchMedia(query).matches;
}

export function useMediaQuery(query: string): boolean {
  const subscribe = useCallback((onChange: () => void) => subscribeTo(query, onChange), [query]);
  const read = useCallback(() => matches(query), [query]);
  return useSyncExternalStore(subscribe, read, () => false);
}

export function useViewportLayout(): ViewportLayout {
  const narrow = useMediaQuery(NARROW_QUERY);
  const wide = useMediaQuery(WIDE_QUERY);
  if (narrow) {
    return 'narrow';
  }
  return wide ? 'wide' : 'medium';
}
