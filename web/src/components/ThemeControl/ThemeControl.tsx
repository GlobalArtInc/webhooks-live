import { useTranslation } from 'react-i18next';
import { applyThemePreference, useThemePreference, type ThemePreference } from '../../prefs/theme';
import styles from './ThemeControl.module.css';

interface ThemeOption {
  value: ThemePreference;
  label: string;
}

export interface ThemeControlProps {
  name?: string;
}

export function ThemeControl({ name = 'theme' }: ThemeControlProps) {
  const { t } = useTranslation();
  const preference = useThemePreference();
  const options: readonly ThemeOption[] = [
    { value: 'system', label: t('wl.theme.system') },
    { value: 'light', label: t('wl.theme.light') },
    { value: 'dark', label: t('wl.theme.dark') },
  ];

  return (
    <div role="radiogroup" aria-label={t('wl.theme.label')} className={styles.control}>
      {options.map((option) => (
        <label key={option.value} className={styles.option}>
          <input
            type="radio"
            name={name}
            value={option.value}
            checked={preference === option.value}
            onChange={() => applyThemePreference(option.value)}
          />
          {option.label}
        </label>
      ))}
    </div>
  );
}
