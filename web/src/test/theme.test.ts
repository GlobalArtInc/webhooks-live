import { describe, expect, it } from 'vitest';
import { resolveThemeAttribute } from '../prefs/theme';

describe('resolveThemeAttribute', () => {
  it('returns null when no preference is stored', () => {
    expect(resolveThemeAttribute(null)).toBeNull();
  });

  it('returns null when the stored value is not valid JSON', () => {
    expect(resolveThemeAttribute('{"theme":')).toBeNull();
  });

  it('returns null when the stored theme is an unknown value', () => {
    expect(resolveThemeAttribute('{"theme":"sepia"}')).toBeNull();
  });

  it.each([
    ['{"theme":"system"}', null],
    ['{"theme":"light"}', 'light'],
    ['{"theme":"dark"}', 'dark'],
  ])('resolves %s to the expected attribute value', (raw, expected) => {
    expect(resolveThemeAttribute(raw)).toBe(expected);
  });
});
