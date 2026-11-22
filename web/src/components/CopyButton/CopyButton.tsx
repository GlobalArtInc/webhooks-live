import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { copyText } from './clipboard';
import styles from './CopyButton.module.css';

type CopyState = 'idle' | 'copied' | 'failed';

interface CopyButtonProps {
  value: string;
  label: string;
  idleText?: string;
  onCopied?: (copied: boolean) => void;
}

const FEEDBACK_MS = 1500;

export function CopyButton({ value, label, idleText, onCopied }: CopyButtonProps) {
  const { t } = useTranslation();
  const [state, setState] = useState<CopyState>('idle');

  useEffect(() => {
    if (state === 'idle') {
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setState('idle');
    }, FEEDBACK_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [state]);

  const text =
    state === 'copied'
      ? t('wl.detail.copy.copied')
      : state === 'failed'
        ? t('wl.detail.copy.failed')
        : (idleText ?? t('wl.detail.copy.idle'));

  return (
    <button
      type="button"
      className={styles.button}
      aria-label={label}
      data-state={state}
      onClick={() => {
        void copyText(value).then((copied) => {
          setState(copied ? 'copied' : 'failed');
          onCopied?.(copied);
        });
      }}
    >
      {text}
    </button>
  );
}
