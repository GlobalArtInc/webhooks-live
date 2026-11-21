import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { TRIGGER_KINDS } from './constants';
import { orderTriggers } from './model';
import styles from './RuleConditionsEditor.module.css';
import type { TriggerKind } from './types';

export interface TriggerKindsProps {
  value: readonly TriggerKind[];
  onChange: (value: TriggerKind[]) => void;
  messages: readonly string[];
  readOnly: boolean;
}

export function TriggerKinds({ value, onChange, messages, readOnly }: TriggerKindsProps) {
  const { t } = useTranslation();
  const errorId = useId();

  function labelOf(kind: TriggerKind): string {
    switch (kind) {
      case 'http':
        return t('wl.conditions.triggers.http');
      case 'email':
        return t('wl.conditions.triggers.email');
      case 'dns':
        return t('wl.conditions.triggers.dns');
    }
  }

  function toggle(kind: TriggerKind, checked: boolean): void {
    onChange(orderTriggers(checked ? [...value, kind] : value.filter((item) => item !== kind)));
  }

  return (
    <fieldset
      className={styles.fieldset}
      data-triggers=""
      aria-describedby={messages.length === 0 ? undefined : errorId}
    >
      <legend className={styles.legend}>{t('wl.conditions.triggers.legend')}</legend>
      <div className={styles.choices}>
        {TRIGGER_KINDS.map((kind) => (
          <label key={kind} className={styles.choice}>
            <input
              type="checkbox"
              checked={value.includes(kind)}
              disabled={readOnly}
              aria-invalid={messages.length > 0}
              onChange={(event) => {
                toggle(kind, event.target.checked);
              }}
            />
            <span>{labelOf(kind)}</span>
          </label>
        ))}
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
