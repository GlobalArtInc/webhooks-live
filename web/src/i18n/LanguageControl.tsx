import { useTranslation } from 'react-i18next';
import { useLanguage } from './provider';
import styles from './LanguageControl.module.css';

const LANGUAGE_NAMES: Readonly<Record<string, string>> = {
  en: 'English',
  ru: 'Русский',
};

export function LanguageControl() {
  const { t } = useTranslation();
  const { language, languages, change } = useLanguage();

  return (
    <label className={styles.control}>
      {t('wl.common.language')}
      <select className={styles.select} value={language} onChange={(event) => void change(event.target.value)}>
        {languages.map((option) => (
          <option key={option} value={option}>
            {LANGUAGE_NAMES[option] ?? option}
          </option>
        ))}
      </select>
    </label>
  );
}
