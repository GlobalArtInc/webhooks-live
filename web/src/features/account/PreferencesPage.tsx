import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ThemeControl } from '../../components/ThemeControl/ThemeControl';
import {
  DEFAULT_DENSITY,
  DEFAULT_TIME_DISPLAY,
  saveDensity,
  saveTimeDisplay,
  useDensity,
  useTimeDisplay,
  type Density,
  type TimeDisplay,
} from '../../prefs/display';
import { resetLayoutPrefs } from '../../prefs/layout';
import { applyThemePreference } from '../../prefs/theme';
import { LanguageControl } from '../../i18n/LanguageControl';
import { useLanguage } from '../../i18n/provider';
import { detectLanguage } from '../../i18n/setup';
import { resetListWidth } from '../list';
import { DEFAULT_LIVE_PREFS, saveLivePrefs, useLivePrefs } from '../live/prefs';
import { DEFAULT_SINGLE_KEY_SHORTCUTS, saveSingleKeyPreference, useSingleKeyPreference } from '../shortcuts/prefs';
import styles from './PreferencesPage.module.css';

interface ChoiceOption<Value extends string> {
  value: Value;
  label: string;
}

interface ChoiceGroupProps<Value extends string> {
  name: string;
  label: string;
  options: readonly ChoiceOption<Value>[];
  value: Value;
  onChange: (value: Value) => void;
}

function ChoiceGroup<Value extends string>({ name, label, options, value, onChange }: ChoiceGroupProps<Value>) {
  const labelId = useId();
  return (
    <div role="radiogroup" aria-labelledby={labelId} className={styles.choices}>
      <span id={labelId} className={styles.label}>
        {label}
      </span>
      {options.map((option) => (
        <label key={option.value} className={styles.choice}>
          <input
            type="radio"
            name={name}
            value={option.value}
            checked={value === option.value}
            onChange={() => onChange(option.value)}
          />
          {option.label}
        </label>
      ))}
    </div>
  );
}

interface SwitchRowProps {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}

function SwitchRow({ label, checked, onChange }: SwitchRowProps) {
  const { t } = useTranslation();
  const labelId = useId();
  return (
    <div className={styles.row}>
      <span id={labelId} className={styles.label}>
        {label}
      </span>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-labelledby={labelId}
        className={styles.switch}
        onClick={() => onChange(!checked)}
      >
        {checked ? t('wl.account.prefs.switch.on') : t('wl.account.prefs.switch.off')}
      </button>
    </div>
  );
}

export function PreferencesPage() {
  const { t } = useTranslation();
  const { languages, change } = useLanguage();
  const density = useDensity();
  const timeDisplay = useTimeDisplay();
  const live = useLivePrefs();
  const singleKey = useSingleKeyPreference();
  const [status, setStatus] = useState('');

  const densityOptions: readonly ChoiceOption<Density>[] = [
    { value: 'comfortable', label: t('wl.account.prefs.density.comfortable') },
    { value: 'compact', label: t('wl.account.prefs.density.compact') },
  ];
  const timeOptions: readonly ChoiceOption<TimeDisplay>[] = [
    { value: 'relative', label: t('wl.account.prefs.time.relative') },
    { value: 'absolute', label: t('wl.account.prefs.time.absolute') },
    { value: 'utc', label: t('wl.account.prefs.time.utc') },
  ];

  function resetAll() {
    applyThemePreference('system');
    saveDensity(DEFAULT_DENSITY);
    saveTimeDisplay(DEFAULT_TIME_DISPLAY);
    saveLivePrefs(DEFAULT_LIVE_PREFS);
    saveSingleKeyPreference(DEFAULT_SINGLE_KEY_SHORTCUTS);
    resetLayoutPrefs();
    resetListWidth();
    void change(detectLanguage(null, navigator.language, languages));
    setStatus(t('wl.account.prefs.reset.done'));
  }

  return (
    <section className={styles.page} aria-labelledby="preferences-title">
      <header className={styles.header}>
        <h1 id="preferences-title" className={styles.title}>
          {t('wl.account.prefs.title')}
        </h1>
        <p className={styles.description}>{t('wl.account.prefs.description')}</p>
      </header>

      <section id="appearance" aria-labelledby="prefs-appearance" className={styles.group}>
        <h2 id="prefs-appearance" className={styles.heading}>
          {t('wl.account.prefs.group.appearance')}
        </h2>
        <div className={styles.row}>
          <span className={styles.label}>{t('wl.theme.label')}</span>
          <ThemeControl name="theme-preference" />
        </div>
        <ChoiceGroup
          name="density"
          label={t('wl.account.prefs.density.label')}
          options={densityOptions}
          value={density}
          onChange={saveDensity}
        />
        <ChoiceGroup
          name="time-display"
          label={t('wl.account.prefs.time.label')}
          options={timeOptions}
          value={timeDisplay}
          onChange={saveTimeDisplay}
        />
      </section>

      <section id="language" aria-labelledby="prefs-language" className={styles.group}>
        <h2 id="prefs-language" className={styles.heading}>
          {t('wl.account.prefs.group.language')}
        </h2>
        <LanguageControl />
      </section>

      <section id="live" aria-labelledby="prefs-live" className={styles.group}>
        <h2 id="prefs-live" className={styles.heading}>
          {t('wl.account.prefs.group.live')}
        </h2>
        <SwitchRow
          label={t('wl.account.prefs.follow.label')}
          checked={live.follow}
          onChange={(follow) => saveLivePrefs({ follow })}
        />
        <SwitchRow
          label={t('wl.account.prefs.announce.label')}
          checked={live.announce}
          onChange={(announce) => saveLivePrefs({ announce })}
        />
      </section>

      <section id="shortcuts" aria-labelledby="prefs-shortcuts" className={styles.group}>
        <h2 id="prefs-shortcuts" className={styles.heading}>
          {t('wl.account.prefs.group.shortcuts')}
        </h2>
        <SwitchRow label={t('wl.shortcuts.singleKey.label')} checked={singleKey} onChange={saveSingleKeyPreference} />
      </section>

      <div className={styles.reset}>
        <button type="button" className={styles.resetButton} onClick={resetAll}>
          {t('wl.account.prefs.reset.button')}
        </button>
        <p role="status" className={styles.status}>
          {status}
        </p>
      </div>
    </section>
  );
}
