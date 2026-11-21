import { afterEach, describe, expect, it } from 'vitest';
import { COMPACT_CLASS, parseDensity, parseTimeDisplay, saveDensity } from './display';
import { PREFS_STORAGE_KEY } from './theme';

afterEach(() => {
  window.localStorage.clear();
  document.documentElement.classList.remove(COMPACT_CLASS);
});

describe('parseDensity', () => {
  it.each([
    [null, 'comfortable'],
    ['{"density":"sepia"}', 'comfortable'],
    ['{"density":"compact"}', 'compact'],
  ])('resolves %j to %j', (raw, expected) => {
    expect(parseDensity(raw)).toBe(expected);
  });
});

describe('parseTimeDisplay', () => {
  it.each([
    [null, 'relative'],
    ['{"timeDisplay":"yesterday"}', 'relative'],
    ['{"timeDisplay":"utc"}', 'utc'],
  ])('resolves %j to %j', (raw, expected) => {
    expect(parseTimeDisplay(raw)).toBe(expected);
  });
});

describe('saveDensity', () => {
  it('keeps the stored theme and applies the compact class at once', () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, '{"theme":"dark"}');

    saveDensity('compact');

    expect(JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? '')).toEqual({
      theme: 'dark',
      density: 'compact',
    });
    expect(document.documentElement.classList.contains(COMPACT_CLASS)).toBe(true);
  });
});
