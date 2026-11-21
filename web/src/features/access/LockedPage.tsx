import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { PASSWORD_MAX } from './secrets';
import styles from './LockedPage.module.css';

export type LockedState = 'idle' | 'checking' | 'wrong' | 'failed' | 'limited';

export interface LockedPageProps {
  state: LockedState;
  secondsLeft: number | null;
  onUnlock: (password: string) => void;
}

export function LockedPage({ state, secondsLeft, onUnlock }: LockedPageProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [password, setPassword] = useState('');
  const inputId = `${baseId}-password`;
  const errorId = `${baseId}-error`;
  const blocked = state === 'checking' || state === 'limited';
  const failed = state === 'wrong' || state === 'failed';

  useEffect(() => {
    if (failed) {
      inputRef.current?.focus();
    }
  }, [failed]);

  function submit(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    if (password === '' || blocked) {
      return;
    }
    const value = password;
    setPassword('');
    onUnlock(value);
  }

  return (
    <section className={styles.page}>
      <h1 className={styles.title}>{t('wl.access.locked.title')}</h1>
      <p className={styles.body}>{t('wl.access.locked.body')}</p>
      <form className={styles.form} onSubmit={submit} noValidate>
        <label htmlFor={inputId} className={styles.label}>
          {t('wl.access.locked.password')}
        </label>
        <input
          ref={inputRef}
          id={inputId}
          type="password"
          className={styles.input}
          autoComplete="current-password"
          autoCapitalize="off"
          spellCheck={false}
          maxLength={PASSWORD_MAX + 1}
          value={password}
          aria-invalid={failed ? true : undefined}
          aria-describedby={failed || state === 'limited' ? errorId : undefined}
          onChange={(event) => {
            setPassword(event.target.value);
          }}
        />
        {state === 'wrong' ? (
          <p id={errorId} role="alert" className={styles.error}>
            {t('wl.access.locked.wrong')}
          </p>
        ) : null}
        {state === 'failed' ? (
          <p id={errorId} role="alert" className={styles.error}>
            {t('wl.access.locked.failed')}
          </p>
        ) : null}
        {state === 'limited' ? (
          <div id={errorId}>
            <p role="alert" className={styles.error}>
              {t('wl.access.locked.limited')}
            </p>
            {secondsLeft === null ? null : (
              <p className={styles.detail}>{t('wl.access.locked.retryIn', { seconds: secondsLeft })}</p>
            )}
          </div>
        ) : null}
        <div className={styles.actions}>
          <button type="submit" className={styles.primary} disabled={blocked}>
            {state === 'checking' ? t('wl.access.locked.checking') : t('wl.access.locked.submit')}
          </button>
        </div>
      </form>
    </section>
  );
}
