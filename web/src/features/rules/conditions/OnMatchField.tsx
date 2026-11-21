import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './RuleConditionsEditor.module.css';
import type { OnMatch } from './types';

export interface OnMatchFieldProps {
  value: OnMatch;
  onChange: (value: OnMatch) => void;
  messages: readonly string[];
  readOnly: boolean;
}

export function OnMatchField({ value, onChange, messages, readOnly }: OnMatchFieldProps) {
  const { t } = useTranslation();
  const name = useId();
  const errorId = useId();

  return (
    <fieldset
      className={styles.fieldset}
      data-on-match=""
      aria-describedby={messages.length === 0 ? undefined : errorId}
    >
      <legend className={styles.legend}>{t('wl.conditions.onMatch.legend')}</legend>
      <div className={styles.choices}>
        <label className={styles.choice}>
          <input
            type="radio"
            name={name}
            checked={value === 'next'}
            disabled={readOnly}
            onChange={() => {
              onChange('next');
            }}
          />
          <span>{t('wl.conditions.onMatch.next')}</span>
          <span className={styles.hint}>{t('wl.conditions.onMatch.nextHint')}</span>
        </label>
        <label className={styles.choice}>
          <input
            type="radio"
            name={name}
            checked={value === 'stop'}
            disabled={readOnly}
            onChange={() => {
              onChange('stop');
            }}
          />
          <span>{t('wl.conditions.onMatch.stop')}</span>
          <span className={styles.hint}>{t('wl.conditions.onMatch.stopHint')}</span>
        </label>
      </div>
      {messages.length === 0 ? null : (
        <ul id={errorId} className={styles.errors}>
          {messages.map((message) => (
            <li key={message} className={styles.error}>
              {message}
            </li>
          ))}
        </ul>
      )}
    </fieldset>
  );
}
