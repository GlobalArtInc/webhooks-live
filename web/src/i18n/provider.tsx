import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { I18nextProvider } from 'react-i18next';
import { PULLED_BUNDLES, type BundleLoaders } from './bundles';
import { loadLanguagePreference, saveLanguagePreference } from './preferences';
import { applyLanguage, availableLanguages, createI18nInstance, detectLanguage, SOURCE_LANGUAGE } from './setup';

export interface LanguageState {
  language: string;
  languages: readonly string[];
  change: (language: string) => Promise<void>;
}

export interface I18nProviderProps {
  children: ReactNode;
  bundles?: BundleLoaders;
}

const LanguageContext = createContext<LanguageState | null>(null);

export function useLanguage(): LanguageState {
  const state = useContext(LanguageContext);
  if (state === null) {
    throw new Error('useLanguage must be used inside I18nProvider');
  }
  return state;
}

export function I18nProvider({ children, bundles = PULLED_BUNDLES }: I18nProviderProps) {
  const [instance] = useState(createI18nInstance);
  const [language, setLanguage] = useState(SOURCE_LANGUAGE);
  const [ready, setReady] = useState(false);
  const languages = useMemo(() => availableLanguages(bundles), [bundles]);

  useEffect(() => {
    let active = true;
    const initial = detectLanguage(loadLanguagePreference(), navigator.language, languages);
    void applyLanguage(instance, initial, bundles).then((effective) => {
      if (active) {
        setLanguage(effective);
        setReady(true);
      }
    });
    return () => {
      active = false;
    };
  }, [instance, bundles, languages]);

  const change = useCallback(
    async (next: string) => {
      const effective = await applyLanguage(instance, next, bundles);
      setLanguage(effective);
      saveLanguagePreference(next);
    },
    [instance, bundles],
  );

  const value = useMemo(() => ({ language, languages, change }), [language, languages, change]);

  if (!ready) {
    return null;
  }

  return (
    <LanguageContext value={value}>
      <I18nextProvider i18n={instance}>{children}</I18nextProvider>
    </LanguageContext>
  );
}
