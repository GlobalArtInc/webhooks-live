import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './RulesToast.module.css';

export const RULES_TOAST_MS = 8000;

export interface RulesToastProps {
  message: string;
  tone: 'info' | 'danger';
  onUndo: (() => void) | null;
  onDismiss: () => void;
}

export function RulesToast({ message, tone, onUndo, onDismiss }: RulesToastProps) {
  const { t } = useTranslation();

  useEffect(() => {
    const timer = window.setTimeout(onDismiss, RULES_TOAST_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [onDismiss]);

  const toneClass = tone === 'danger' ? `${styles.toast} ${styles.danger}` : styles.toast;
  return (
    <div className={toneClass} role={tone === 'danger' ? 'alert' : 'status'}>
      <span className={styles.message}>{message}</span>
      {onUndo === null ? null : (
        <button type="button" className={styles.action} onClick={onUndo}>
          {t('wl.rules.undo.action')}
        </button>
      )}
      <button type="button" className={styles.dismiss} onClick={onDismiss}>
        {t('wl.rules.undo.dismiss')}
      </button>
    </div>
  );
}
