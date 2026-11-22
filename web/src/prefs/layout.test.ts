import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  MAX_CLOSED_GROUPS,
  parseBodyView,
  parseClosedGroups,
  parseRail,
  resetLayoutPrefs,
  saveBodyView,
  saveGroupOpen,
  saveRail,
  toggleBodyMode,
  useBodyViewPreference,
  useRailPreference,
} from './layout';
import { PREFS_STORAGE_KEY } from './theme';

afterEach(() => {
  window.localStorage.clear();
});

function stored(): unknown {
  return JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? 'null');
}

describe('parseRail', () => {
  it.each([
    [null, 'expanded'],
    ['{"theme":"dark"}', 'expanded'],
    ['{"rail":"narrow"}', 'expanded'],
    ['{"rail":7}', 'expanded'],
    ['not json', 'expanded'],
    ['{"rail":"collapsed"}', 'collapsed'],
  ])('resolves %j to %j', (raw, expected) => {
    expect(parseRail(raw)).toBe(expected);
  });
});

describe('parseBodyView', () => {
  it.each([
    [null, { mode: 'pretty', wrap: false }],
    ['{"bodyMode":"hex","bodyWrap":"yes"}', { mode: 'pretty', wrap: false }],
    ['{"bodyMode":"raw","bodyWrap":true}', { mode: 'raw', wrap: true }],
    ['{"bodyMode":"raw"}', { mode: 'raw', wrap: false }],
  ])('resolves %j to %j', (raw, expected) => {
    expect(parseBodyView(raw)).toEqual(expected);
  });
});

describe('parseClosedGroups', () => {
  it('keeps only non-empty strings and caps the list', () => {
    expect(parseClosedGroups('{"railGroupsClosed":"x"}')).toEqual([]);
    expect(parseClosedGroups('{"railGroupsClosed":["a",1,"",null,"b"]}')).toEqual(['a', 'b']);
    const many = JSON.stringify({ railGroupsClosed: Array.from({ length: MAX_CLOSED_GROUPS + 5 }, (_, i) => `k${i}`) });
    expect(parseClosedGroups(many)).toHaveLength(MAX_CLOSED_GROUPS);
  });
});

describe('saving', () => {
  it('keeps the other stored fields and an old file without the new ones still loads', () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, '{"theme":"dark","density":"compact"}');

    saveRail('collapsed');
    saveBodyView({ mode: 'raw' });
    saveBodyView({ wrap: true });
    saveGroupOpen('local:a', false);
    saveGroupOpen('local:b', false);
    saveGroupOpen('local:a', true);

    expect(stored()).toEqual({
      theme: 'dark',
      density: 'compact',
      rail: 'collapsed',
      bodyMode: 'raw',
      bodyWrap: true,
      railGroupsClosed: ['local:b'],
    });
  });

  it('resets only the layout fields', () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, '{"theme":"dark"}');
    saveRail('collapsed');
    saveBodyView({ mode: 'raw', wrap: true });
    saveGroupOpen('x', false);

    resetLayoutPrefs();

    expect(stored()).toEqual({ theme: 'dark' });
  });
});

describe('denied storage', () => {
  it('keeps the view choices for the rest of the session when a write throws', () => {
    const denied = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new DOMException('denied', 'SecurityError');
    });
    try {
      const rail = renderHook(() => useRailPreference());
      const body = renderHook(() => useBodyViewPreference());
      expect(rail.result.current).toBe('expanded');

      act(() => {
        saveRail('collapsed');
        toggleBodyMode();
      });

      expect(rail.result.current).toBe('collapsed');
      expect(body.result.current.mode).toBe('raw');
    } finally {
      denied.mockRestore();
      saveRail('expanded');
    }
  });
});
