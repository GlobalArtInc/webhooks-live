import { describe, expect, it } from 'vitest';
import { moveItem, targetIndex } from './order';

describe('targetIndex', () => {
  it('moves one place up or down and stops at the ends', () => {
    expect(targetIndex(1, 'up', 3)).toBe(0);
    expect(targetIndex(0, 'up', 3)).toBe(0);
    expect(targetIndex(1, 'down', 3)).toBe(2);
    expect(targetIndex(2, 'down', 3)).toBe(2);
  });

  it('moves to the top from any place', () => {
    expect(targetIndex(2, 'top', 3)).toBe(0);
  });
});

describe('moveItem', () => {
  it('moves an item to a new index and keeps the rest in order', () => {
    expect(moveItem(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
    expect(moveItem(['a', 'b', 'c'], 0, 2)).toEqual(['b', 'c', 'a']);
  });

  it('returns a copy unchanged when the source index is out of range', () => {
    expect(moveItem(['a'], 3, 0)).toEqual(['a']);
  });
});
