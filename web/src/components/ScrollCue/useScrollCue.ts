import { useCallback, useEffect, useRef, useState, type RefObject } from 'react';

const REVEAL_MARGIN = 32;
const EDGE_TOLERANCE = 1;
const SELECTED_SELECTOR = '[aria-current]';

export interface ScrollEdges {
  start: boolean;
  end: boolean;
}

export interface ScrollCue<T extends HTMLElement> {
  ref: RefObject<T | null>;
  edges: ScrollEdges;
}

const NO_EDGES: ScrollEdges = { start: false, end: false };

export function readEdges(element: HTMLElement): ScrollEdges {
  return {
    start: element.scrollLeft > EDGE_TOLERANCE,
    end: element.scrollLeft + element.clientWidth < element.scrollWidth - EDGE_TOLERANCE,
  };
}

export function revealOffset(
  itemLeft: number,
  itemRight: number,
  viewLeft: number,
  viewRight: number,
  margin: number,
): number {
  if (itemLeft < viewLeft + margin) {
    return itemLeft - viewLeft - margin;
  }
  if (itemRight > viewRight - margin) {
    return itemRight - viewRight + margin;
  }
  return 0;
}

export function useScrollCue<T extends HTMLElement>(selectedKey: string | null): ScrollCue<T> {
  const ref = useRef<T>(null);
  const [edges, setEdges] = useState<ScrollEdges>(NO_EDGES);

  const measure = useCallback(() => {
    const element = ref.current;
    if (element === null) {
      return;
    }
    const next = readEdges(element);
    setEdges((current) => (current.start === next.start && current.end === next.end ? current : next));
  }, []);

  useEffect(() => {
    const element = ref.current;
    if (element === null) {
      return undefined;
    }
    measure();
    element.addEventListener('scroll', measure, { passive: true });
    window.addEventListener('resize', measure);
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(element);
    return () => {
      element.removeEventListener('scroll', measure);
      window.removeEventListener('resize', measure);
      observer?.disconnect();
    };
  }, [measure]);

  useEffect(() => {
    const element = ref.current;
    const selected = element?.querySelector<HTMLElement>(SELECTED_SELECTOR) ?? null;
    if (element === null || selected === null) {
      return;
    }
    const view = element.getBoundingClientRect();
    const item = selected.getBoundingClientRect();
    const shift = revealOffset(item.left, item.right, view.left, view.right, REVEAL_MARGIN);
    if (shift !== 0) {
      element.scrollLeft += shift;
    }
    measure();
  }, [selectedKey, measure]);

  return { ref, edges };
}
