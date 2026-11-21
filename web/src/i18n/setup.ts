import { createInstance, type i18n as I18nInstance } from 'i18next';
import type { BundleLoader, BundleLoaders, Dictionary } from './bundles';
import common from './en/common.json';
import detail from './en/detail.json';
import diff from './en/diff.json';
import endpoint from './en/endpoint.json';
import exportMessages from './en/export.json';
import forwarding from './en/forwarding.json';
import home from './en/home.json';
import live from './en/live.json';
import sample from './en/sample.json';
import state from './en/state.json';
import workspace from './en/workspace.json';

export const SOURCE_LANGUAGE = 'en';

export function createI18nInstance(): I18nInstance {
  const instance = createInstance();
  void instance.init({
    lng: SOURCE_LANGUAGE,
    fallbackLng: SOURCE_LANGUAGE,
    initAsync: false,
    keySeparator: false,
    resources: {
      [SOURCE_LANGUAGE]: {
        translation: {
          ...common,
          ...detail,
          ...diff,
          ...endpoint,
          ...exportMessages,
          ...forwarding,
          ...home,
          ...live,
          ...sample,
          ...state,
          ...workspace,
        },
      },
    },
    interpolation: { escapeValue: false },
  });
  return instance;
}

export function availableLanguages(bundles: BundleLoaders): string[] {
  return [SOURCE_LANGUAGE, ...Object.keys(bundles).sort()];
}

export function detectLanguage(stored: string | null, browser: string, available: readonly string[]): string {
  const [browserBase = ''] = browser.split('-');
  const candidates = [stored, browserBase];
  return (
    candidates.find((candidate): candidate is string => candidate !== null && available.includes(candidate)) ??
    SOURCE_LANGUAGE
  );
}

function isDictionary(value: unknown): value is Dictionary {
  return (
    typeof value === 'object' &&
    value !== null &&
    !Array.isArray(value) &&
    Object.values(value).every((text) => typeof text === 'string')
  );
}

async function loadBundle(loader: BundleLoader): Promise<Dictionary | null> {
  try {
    const value = await loader();
    return isDictionary(value) ? value : null;
  } catch {
    return null;
  }
}

export async function applyLanguage(instance: I18nInstance, language: string, bundles: BundleLoaders): Promise<string> {
  const loader = bundles[language];
  if (language !== SOURCE_LANGUAGE && loader !== undefined) {
    const bundle = await loadBundle(loader);
    if (bundle !== null) {
      instance.addResourceBundle(language, 'translation', bundle, true, true);
      await instance.changeLanguage(language);
      return language;
    }
  }
  await instance.changeLanguage(SOURCE_LANGUAGE);
  return SOURCE_LANGUAGE;
}
