import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { applyThemePreference, loadThemePreference, type ThemePreference } from '../../prefs/theme';
import styles from './ThemeControl.module.css';

interface ThemeOption {
  value: ThemePreference;
  label: string;
}

export function ThemeControl() {
  const { t } = useTranslation();
  const [preference, setPreference] = useState<ThemePreference>(loadThemePreference);
  const options: readonly ThemeOption[] = [
    { value: 'system', label: t('wl.theme.system') },
    { value: 'light', label: t('wl.theme.light') },
    { value: 'dark', label: t('wl.theme.dark') },
  ];

  function choose(next: ThemePreference) {
    applyThemePreference(next);
    setPreference(next);
  }

  return (
    <div role="radiogroup" aria-label={t('wl.theme.label')} className={styles.control}>
      {options.map((option) => (
        <label key={option.value} className={styles.option}>
          <input
            type="radio"
            name="theme"
            value={option.value}
            checked={preference === option.value}
            onChange={() => choose(option.value)}
          />
          {option.label}
        </label>
      ))}
    </div>
  );
}
