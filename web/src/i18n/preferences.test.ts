import { afterEach, describe, expect, it } from 'vitest';
import { PREFS_STORAGE_KEY } from '../prefs/theme';
import { readLanguagePreference, saveLanguagePreference } from './preferences';

afterEach(() => {
  window.localStorage.clear();
});

describe('readLanguagePreference', () => {
  it.each([
    [null, null],
    ['{"language":', null],
    ['{"language":3}', null],
    ['{"language":"ru"}', 'ru'],
  ])('resolves %s to %s', (raw, expected) => {
    expect(readLanguagePreference(raw)).toBe(expected);
  });
});

describe('saveLanguagePreference', () => {
  it('writes the language next to the theme already stored under the prefs key', () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, '{"theme":"dark"}');

    saveLanguagePreference('ru');

    expect(JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? '')).toEqual({ theme: 'dark', language: 'ru' });
  });
});
