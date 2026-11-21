import { act, cleanup, render, screen } from '@testing-library/react';
import { useRef } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DetailSectionId } from './types';
import { useActiveSection } from './useActiveSection';

const SECTION_IDS: readonly DetailSectionId[] = ['summary', 'body'];

class FakeIntersectionObserver {
  static instances: FakeIntersectionObserver[] = [];
  readonly elements: Element[] = [];
  private readonly callback: IntersectionObserverCallback;

  constructor(callback: IntersectionObserverCallback) {
    this.callback = callback;
    FakeIntersectionObserver.instances.push(this);
  }

  observe(element: Element): void {
    this.elements.push(element);
  }

  unobserve(): void {}

  disconnect(): void {}

  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }

  report(entries: IntersectionObserverEntry[]): void {
    this.callback(entries, this as unknown as IntersectionObserver);
  }
}

function entryFor(target: Element, isIntersecting: boolean): IntersectionObserverEntry {
  return { target, isIntersecting } as IntersectionObserverEntry;
}

function Probe() {
  const rootRef = useRef<HTMLDivElement>(null);
  const active = useActiveSection(rootRef, SECTION_IDS);
  return (
    <div ref={rootRef}>
      <section data-section="summary" />
      <section data-section="body" />
      <output>{active ?? 'none'}</output>
    </div>
  );
}

beforeEach(() => {
  FakeIntersectionObserver.instances = [];
  vi.stubGlobal('IntersectionObserver', FakeIntersectionObserver);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe('useActiveSection', () => {
  it('highlights the first intersecting section and clears it when none intersects', () => {
    render(<Probe />);
    const observer = FakeIntersectionObserver.instances[0];
    const [summary, body] = observer?.elements ?? [];
    if (observer === undefined || summary === undefined || body === undefined) {
      throw new Error('the sections were not observed');
    }

    act(() => {
      observer.report([entryFor(summary, false), entryFor(body, true)]);
    });
    expect(screen.getByRole('status').textContent).toBe('body');

    act(() => {
      observer.report([entryFor(body, false)]);
    });
    expect(screen.getByRole('status').textContent).toBe('none');
  });
});
