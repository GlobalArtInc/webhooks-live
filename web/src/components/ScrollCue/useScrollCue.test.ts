import { describe, expect, it } from 'vitest';
import { readEdges, revealOffset } from './useScrollCue';

function strip(scrollLeft: number, clientWidth: number, scrollWidth: number): HTMLElement {
  const element = document.createElement('ul');
  Object.defineProperty(element, 'scrollLeft', { value: scrollLeft });
  Object.defineProperty(element, 'clientWidth', { value: clientWidth });
  Object.defineProperty(element, 'scrollWidth', { value: scrollWidth });
  return element;
}

describe('scroll edges of a tab strip', () => {
  it('reports hidden content on the side that has some', () => {
    expect(readEdges(strip(0, 300, 600))).toEqual({ start: false, end: true });
    expect(readEdges(strip(150, 300, 600))).toEqual({ start: true, end: true });
    expect(readEdges(strip(300, 300, 600))).toEqual({ start: true, end: false });
  });

  it('reports no cue when everything fits', () => {
    expect(readEdges(strip(0, 300, 300))).toEqual({ start: false, end: false });
    expect(readEdges(strip(0, 300, 300.5))).toEqual({ start: false, end: false });
  });
});

describe('scrolling the selected tab into view', () => {
  const margin = 32;

  it('does nothing for a tab that is fully visible clear of the cue', () => {
    expect(revealOffset(60, 140, 0, 300, margin)).toBe(0);
  });

  it('scrolls forward until the tab clears the right cue and back until it clears the left one', () => {
    expect(revealOffset(250, 340, 0, 300, margin)).toBe(72);
    expect(revealOffset(10, 90, 0, 300, margin)).toBe(-22);
  });
});
