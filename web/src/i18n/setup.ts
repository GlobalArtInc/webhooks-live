import { createInstance, type i18n as I18nInstance } from 'i18next';
import type { BundleLoader, BundleLoaders, Dictionary } from './bundles';
import access from './en/access.json';
import common from './en/common.json';
import composer from './en/composer.json';
import conditions from './en/conditions.json';
import detail from './en/detail.json';
import diff from './en/diff.json';
import editor from './en/editor.json';
import email from './en/email.json';
import endpoint from './en/endpoint.json';
import exportMessages from './en/export.json';
import exportDialog from './en/exportdialog.json';
import forwarding from './en/forwarding.json';
import home from './en/home.json';
import kinds from './en/kinds.json';
import inbox from './en/inbox.json';
import live from './en/live.json';
import rules from './en/rules.json';
import runlogs from './en/runlogs.json';
import rail from './en/rail.json';
import sample from './en/sample.json';
import script from './en/script.json';
import selection from './en/selection.json';
import schedules from './en/schedules.json';
import state from './en/state.json';
import steps from './en/steps.json';
import tokens from './en/tokens.json';
import templating from './en/templating.json';
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
          ...access,
          ...common,
          ...composer,
          ...conditions,
          ...detail,
          ...diff,
          ...editor,
          ...email,
          ...endpoint,
          ...exportMessages,
          ...exportDialog,
          ...forwarding,
          ...home,
          ...kinds,
          ...inbox,
          ...live,
          ...rules,
          ...runlogs,
          ...rail,
          ...sample,
          ...script,
          ...selection,
          ...schedules,
          ...state,
          ...steps,
          ...tokens,
          ...templating,
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
