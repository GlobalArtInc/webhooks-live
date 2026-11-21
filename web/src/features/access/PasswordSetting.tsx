import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PASSWORD_MAX } from './secrets';
import type { AccessDraft, AccessIssue, PasswordAction } from './types';
import { useAccessIssueText } from './issueText';
import styles from './access.module.css';

export interface PasswordSettingProps {
  draft: AccessDraft;
  isSet: boolean;
  readOnly: boolean;
  issue: AccessIssue | null;
  onChange: (draft: AccessDraft) => void;
}

export function PasswordSetting({ draft, isSet, readOnly, issue, onChange }: PasswordSettingProps) {
  const { t } = useTranslation();
  const issueText = useAccessIssueText();
  const baseId = useId();
  const newRef = useRef<HTMLInputElement>(null);
  const [reveal, setReveal] = useState(false);
  const editing = draft.passwordAction === 'set';
  const newId = `${baseId}-new`;
  const confirmId = `${baseId}-confirm`;
  const errorId = `${baseId}-error`;
  const message = issue === null ? null : issueText(issue);

  if (!editing && reveal) {
    setReveal(false);
  }

  useEffect(() => {
    if (editing) {
      newRef.current?.focus();
    }
  }, [editing]);

  function choose(action: PasswordAction): void {
    setReveal(false);
    onChange({ ...draft, passwordAction: action, password: '', confirmPassword: '' });
  }

  return (
    <fieldset className={styles.block} disabled={readOnly}>
      <legend className={styles.blockTitle}>{t('wl.access.password.title')}</legend>
      <p className={styles.hint}>{t('wl.access.password.description')}</p>
      <p className={styles.status}>
        {draft.passwordAction === 'remove'
          ? t('wl.access.password.willRemove')
          : isSet
            ? t('wl.access.password.isSet')
            : t('wl.access.password.isNotSet')}
      </p>
      {editing ? (
        <div className={styles.fields}>
          <div className={styles.field}>
            <label htmlFor={newId} className={styles.label}>
              {t('wl.access.password.new')}
            </label>
            <input
              ref={newRef}
              id={newId}
              type={reveal ? 'text' : 'password'}
              className={styles.input}
              autoComplete="new-password"
              autoCapitalize="off"
              spellCheck={false}
              maxLength={PASSWORD_MAX + 1}
              value={draft.password}
              aria-invalid={message !== null ? true : undefined}
              aria-describedby={message !== null ? errorId : undefined}
              onChange={(event) => {
                onChange({ ...draft, password: event.target.value });
              }}
            />
          </div>
          <div className={styles.field}>
            <label htmlFor={confirmId} className={styles.label}>
              {t('wl.access.password.confirm')}
            </label>
            <input
              id={confirmId}
              type={reveal ? 'text' : 'password'}
              className={styles.input}
              autoComplete="new-password"
              autoCapitalize="off"
              spellCheck={false}
              maxLength={PASSWORD_MAX + 1}
              value={draft.confirmPassword}
              aria-invalid={message !== null ? true : undefined}
              aria-describedby={message !== null ? errorId : undefined}
              onChange={(event) => {
                onChange({ ...draft, confirmPassword: event.target.value });
              }}
            />
          </div>
          {message === null ? null : (
            <p id={errorId} role="alert" className={styles.error}>
              {message}
            </p>
          )}
        </div>
      ) : null}
      {readOnly ? null : (
        <div className={styles.actions}>
          {editing ? (
            <>
              <button
                type="button"
                className={styles.button}
                aria-pressed={reveal}
                onClick={() => {
                  setReveal((current) => !current);
                }}
              >
                {reveal ? t('wl.access.password.hide') : t('wl.access.password.show')}
              </button>
              <button
                type="button"
                className={styles.button}
                onClick={() => {
                  choose('keep');
                }}
              >
                {t('wl.access.password.cancel')}
              </button>
            </>
          ) : null}
          {!editing && draft.passwordAction === 'keep' ? (
            <button
              type="button"
              className={styles.button}
              onClick={() => {
                choose('set');
              }}
            >
              {isSet ? t('wl.access.password.change') : t('wl.access.password.set')}
            </button>
          ) : null}
          {!editing && draft.passwordAction === 'keep' && isSet ? (
            <button
              type="button"
              className={styles.danger}
              onClick={() => {
                choose('remove');
              }}
            >
              {t('wl.access.password.remove')}
            </button>
          ) : null}
          {draft.passwordAction === 'remove' ? (
            <button
              type="button"
              className={styles.button}
              onClick={() => {
                choose('keep');
              }}
            >
              {t('wl.access.password.keep')}
            </button>
          ) : null}
        </div>
      )}
    </fieldset>
  );
}
