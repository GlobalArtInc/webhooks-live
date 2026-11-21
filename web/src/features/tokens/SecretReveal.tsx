import { useEffect, useId, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { copyText } from '../../components/CopyButton/clipboard';
import styles from './SecretReveal.module.css';

export interface SecretRevealProps {
  secret: string;
  onCopied: () => void;
}

type CopyState = 'idle' | 'copied' | 'failed';

const MASK_CHARACTER = '•';

export function SecretReveal({ secret, onCopied }: SecretRevealProps) {
  const { t } = useTranslation();
  const fieldRef = useRef<HTMLInputElement>(null);
  const onceId = useId();
  const statusId = useId();
  const [shown, setShown] = useState(false);
  const [copyState, setCopyState] = useState<CopyState>('idle');

  useEffect(() => {
    fieldRef.current?.focus();
  }, []);

  function copy() {
    void copyText(secret).then((copied) => {
      setCopyState(copied ? 'copied' : 'failed');
      if (copied) {
        onCopied();
      }
    });
  }

  const displayed = shown ? secret : MASK_CHARACTER.repeat(secret.length);
  const copyLabel = copyState === 'copied' ? t('wl.tokens.secret.copied') : t('wl.tokens.secret.copy');

  return (
    <div className={styles.root}>
      <p id={onceId} className={styles.once}>
        {t('wl.tokens.secret.once')}
      </p>
      <div className={styles.row}>
        <input
          ref={fieldRef}
          className={styles.field}
          type="text"
          readOnly
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="off"
          aria-label={t('wl.tokens.secret.label')}
          aria-describedby={`${onceId} ${statusId}`}
          value={displayed}
        />
        <button type="button" className={styles.button} onClick={() => setShown((current) => !current)}>
          {shown ? t('wl.tokens.secret.hide') : t('wl.tokens.secret.show')}
        </button>
        <button type="button" className={styles.button} data-state={copyState} onClick={copy}>
          {copyLabel}
        </button>
      </div>
      <p id={statusId} role="status" className={styles.status}>
        {copyState === 'failed' ? t('wl.tokens.secret.copyFailed') : ''}
      </p>
      <p className={styles.usage}>{t('wl.tokens.secret.usage')}</p>
    </div>
  );
}
