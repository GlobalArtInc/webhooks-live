import { afterEach, describe, expect, it } from 'vitest';
import { PREFS_STORAGE_KEY } from '../../prefs/theme';
import { LIST_WIDTH_DEFAULT, LIST_WIDTH_MAX, LIST_WIDTH_MIN, parseListWidth, saveListWidth } from './listWidth';

describe('list width preference', () => {
  afterEach(() => {
    window.localStorage.clear();
  });

  it('falls back to the default for missing, unreadable or non-numeric values and clamps the rest', () => {
    expect(parseListWidth(null)).toBe(LIST_WIDTH_DEFAULT);
    expect(parseListWidth('not json')).toBe(LIST_WIDTH_DEFAULT);
    expect(parseListWidth(JSON.stringify({ listWidth: 'wide' }))).toBe(LIST_WIDTH_DEFAULT);
    expect(parseListWidth(JSON.stringify({ listWidth: 10 }))).toBe(LIST_WIDTH_MIN);
    expect(parseListWidth(JSON.stringify({ listWidth: 9000 }))).toBe(LIST_WIDTH_MAX);
  });

  it('saves the width next to the other preference fields without dropping them', () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify({ theme: 'dark' }));

    saveListWidth(420.4);

    expect(JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? '{}')).toEqual({
      theme: 'dark',
      listWidth: 420,
    });
  });
});
