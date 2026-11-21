import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { operatorLabel } from './guardText';
import {
  GUARD_OPERATORS,
  changeGuardOperator,
  createGuard,
  guardArgumentFromText,
  guardArgumentToText,
  operatorNeedsArgument,
} from './model';
import styles from './StepGuardEditor.module.css';
import type { GuardOperator, GuardPredicate } from './types';

export interface StepGuardEditorProps {
  guard: GuardPredicate | null;
  onChange: (guard: GuardPredicate | null) => void;
  idPrefix: string;
  error?: string | null;
  readOnly?: boolean;
}

export function StepGuardEditor({ guard, onChange, idPrefix, error = null, readOnly = false }: StepGuardEditorProps) {
  const { t } = useTranslation();
  const errorId = `${idPrefix}-guard-error`;
  const describedBy = error === null ? undefined : errorId;
  const [argumentText, setArgumentText] = useState(() => guardArgumentToText(guard?.arg ?? null));

  if (guard !== null && JSON.stringify(guardArgumentFromText(guard.op, argumentText)) !== JSON.stringify(guard.arg)) {
    setArgumentText(guardArgumentToText(guard.arg));
  }

  return (
    <fieldset className={styles.guard} data-schema-field="guard" data-invalid={error !== null}>
      <legend className={styles.legend}>{t('wl.steps.guard.legend')}</legend>
      <label className={styles.toggle}>
        <input
          type="checkbox"
          checked={guard !== null}
          disabled={readOnly}
          onChange={(event) => {
            onChange(event.target.checked ? createGuard() : null);
          }}
        />
        <span>{t('wl.steps.guard.toggle')}</span>
      </label>
      {guard === null ? null : (
        <div className={styles.row}>
          <label className={styles.cell}>
            <span className={styles.label}>{t('wl.steps.guard.subject')}</span>
            <input
              type="text"
              className={styles.control}
              data-mono="true"
              value={guard.value}
              placeholder={t('wl.steps.guard.subjectPlaceholder')}
              readOnly={readOnly}
              autoComplete="off"
              autoCapitalize="off"
              spellCheck={false}
              aria-invalid={error !== null}
              aria-describedby={describedBy}
              onChange={(event) => {
                onChange({ ...guard, value: event.target.value });
              }}
            />
          </label>
          <label className={styles.cell}>
            <span className={styles.label}>{t('wl.steps.guard.operator')}</span>
            <select
              className={styles.control}
              value={guard.op}
              disabled={readOnly}
              onChange={(event) => {
                onChange(changeGuardOperator(guard, event.target.value as GuardOperator));
              }}
            >
              {GUARD_OPERATORS.map((op) => (
                <option key={op} value={op}>
                  {operatorLabel(t, op)}
                </option>
              ))}
            </select>
          </label>
          {operatorNeedsArgument(guard.op) ? (
            <label className={styles.cell}>
              <span className={styles.label}>{t('wl.steps.guard.argument')}</span>
              <input
                type="text"
                className={styles.control}
                data-mono="true"
                value={argumentText}
                placeholder={
                  guard.op === 'in' || guard.op === 'in_cidr' ? t('wl.steps.guard.listPlaceholder') : undefined
                }
                readOnly={readOnly}
                autoComplete="off"
                spellCheck={false}
                aria-invalid={error !== null}
                aria-describedby={describedBy}
                onChange={(event) => {
                  setArgumentText(event.target.value);
                  onChange({ ...guard, arg: guardArgumentFromText(guard.op, event.target.value) });
                }}
              />
            </label>
          ) : null}
        </div>
      )}
      {error === null ? null : (
        <p id={errorId} className={styles.error}>
          {error}
        </p>
      )}
    </fieldset>
  );
}
