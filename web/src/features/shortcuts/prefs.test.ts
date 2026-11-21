import { afterEach, describe, expect, it } from 'vitest';
import { PREFS_STORAGE_KEY } from '../../prefs/theme';
import { parseSingleKeyPreference, saveSingleKeyPreference } from './prefs';

afterEach(() => {
  window.localStorage.clear();
});

describe('parseSingleKeyPreference', () => {
  it.each([
    [null, true],
    ['{"singleKeyShortcuts":', true],
    ['{"singleKeyShortcuts":"no"}', true],
    ['{"singleKeyShortcuts":false}', false],
  ])('resolves %s to %s', (raw, expected) => {
    expect(parseSingleKeyPreference(raw)).toBe(expected);
  });
});

describe('saveSingleKeyPreference', () => {
  it('writes the flag next to the theme already stored under the prefs key', () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, '{"theme":"dark"}');

    saveSingleKeyPreference(false);

    expect(JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? '')).toEqual({
      theme: 'dark',
      singleKeyShortcuts: false,
    });
  });
});
