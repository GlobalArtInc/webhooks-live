import { useEffect, useState, type RefObject } from 'react';
import type { DetailSectionId } from './types';

const OBSERVER_OPTIONS: IntersectionObserverInit = { rootMargin: '-64px 0px -60% 0px', threshold: 0 };
const SECTION_SELECTOR = '[data-section]';

export function useActiveSection(
  rootRef: RefObject<HTMLElement | null>,
  sectionIds: readonly DetailSectionId[],
): DetailSectionId | null {
  const [active, setActive] = useState<DetailSectionId | null>(null);

  useEffect(() => {
    const root = rootRef.current;
    if (root === null || typeof IntersectionObserver === 'undefined') {
      return undefined;
    }
    const intersecting = new Map<DetailSectionId, boolean>();
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const id = sectionIds.find((candidate) => candidate === entry.target.getAttribute('data-section'));
        if (id !== undefined) {
          intersecting.set(id, entry.isIntersecting);
        }
      }
      setActive(sectionIds.find((candidate) => intersecting.get(candidate) === true) ?? null);
    }, OBSERVER_OPTIONS);
    root.querySelectorAll(SECTION_SELECTOR).forEach((element) => {
      observer.observe(element);
    });
    return () => {
      observer.disconnect();
    };
  }, [rootRef, sectionIds]);

  return active;
}
