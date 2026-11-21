import { describe, expect, it } from 'vitest';
import ruFixture from '../test/i18n-ru.json';
import type { BundleLoaders } from './bundles';
import { applyLanguage, createI18nInstance, detectLanguage } from './setup';

const RU_BUNDLES: BundleLoaders = { ru: () => Promise.resolve(ruFixture) };

describe('applyLanguage', () => {
  it('falls back to English when the language bundle is absent', async () => {
    const instance = createI18nInstance();

    expect(await applyLanguage(instance, 'ru', {})).toBe('en');
    expect(instance.t('wl.sample.title')).toBe('Design tokens');
  });

  it.each([
    [1, '1 new capture'],
    [2, '2 new captures'],
    [5, '5 new captures'],
  ])('selects the English plural form for %i', (count, expected) => {
    expect(createI18nInstance().t('wl.sample.newCaptures', { count })).toBe(expected);
  });

  it.each([
    [1, '1 перехват'],
    [2, '2 перехвата'],
    [5, '5 перехватов'],
  ])('selects the Russian plural form for %i', async (count, expected) => {
    const instance = createI18nInstance();
    await applyLanguage(instance, 'ru', RU_BUNDLES);

    expect(instance.t('wl.sample.newCaptures', { count })).toBe(expected);
  });
});

describe('detectLanguage', () => {
  it('prefers the stored language, then the browser language, then English', () => {
    expect(detectLanguage('ru', 'en-US', ['en', 'ru'])).toBe('ru');
    expect(detectLanguage(null, 'ru-RU', ['en', 'ru'])).toBe('ru');
    expect(detectLanguage('de', 'de-DE', ['en', 'ru'])).toBe('en');
    expect(detectLanguage('ru', 'ru-RU', ['en'])).toBe('en');
  });
});
