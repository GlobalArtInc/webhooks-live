import { useState } from 'react';
import { THEME_PREFERENCES, applyThemePreference, loadThemePreference, type ThemePreference } from '../prefs/theme';
import styles from './ThemeControl.module.css';

const THEME_LABELS: Record<ThemePreference, string> = {
  system: 'System',
  light: 'Light',
  dark: 'Dark',
};

export function ThemeControl() {
  const [preference, setPreference] = useState<ThemePreference>(loadThemePreference);

  function choose(next: ThemePreference) {
    applyThemePreference(next);
    setPreference(next);
  }

  return (
    <div role="radiogroup" aria-label="Theme" className={styles.control}>
      {THEME_PREFERENCES.map((option) => (
        <label key={option} className={styles.option}>
          <input
            type="radio"
            name="theme"
            value={option}
            checked={preference === option}
            onChange={() => choose(option)}
          />
          {THEME_LABELS[option]}
        </label>
      ))}
    </div>
  );
}
