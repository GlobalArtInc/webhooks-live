import { describe, expect, it } from 'vitest';
import { displayKey, findShortcut, SHORTCUTS, type ShortcutFilter } from './registry';

const EVERY_SCOPE: ShortcutFilter = { singleKey: true, isActive: () => true };
const ENDPOINT_ONLY: ShortcutFilter = { singleKey: true, isActive: (scope) => scope === 'endpoint' };

describe('findShortcut', () => {
  it('tells Shift+K from k by the case of the key', () => {
    expect(findShortcut('K', EVERY_SCOPE)?.id).toBe('newest');
    expect(findShortcut('k', EVERY_SCOPE)?.id).toBe('previous');
  });

  it('resolves a binding only while its scope is active', () => {
    expect(findShortcut('y', ENDPOINT_ONLY)).toBeUndefined();
    expect(findShortcut('j', ENDPOINT_ONLY)?.id).toBe('next');
  });

  it('drops character keys when single-key shortcuts are off, and keeps Escape', () => {
    const off: ShortcutFilter = { singleKey: false, isActive: () => true };

    expect(findShortcut('j', off)).toBeUndefined();
    expect(findShortcut('?', off)).toBeUndefined();
    expect(findShortcut('Escape', off)?.id).toBe('close');
    expect(findShortcut('Delete', off)?.id).toBe('delete');
  });
});

describe('displayKey', () => {
  it.each([
    ['K', 'Shift+K'],
    ['Escape', 'Esc'],
    ['ArrowDown', 'Down'],
    ['j', 'j'],
  ])('shows %s as %s', (key, expected) => {
    expect(displayKey(key)).toBe(expected);
  });
});

describe('SHORTCUTS', () => {
  it('binds each key to one action within a scope', () => {
    const seen = new Set<string>();
    for (const entry of SHORTCUTS) {
      for (const key of entry.keys) {
        const slot = `${entry.scope}:${key}`;
        expect(seen.has(slot), slot).toBe(false);
        seen.add(slot);
      }
    }
  });
});
