import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import styles from './CopyAction.module.css';

export interface CopyActionProps {
  value: string;
  label: string;
}

type Outcome = 'idle' | 'copied' | 'failed';

const ANNOUNCEMENT_MS = 4000;

export function CopyAction({ value, label }: CopyActionProps) {
  const { t } = useTranslation();
  const [outcome, setOutcome] = useState<Outcome>('idle');

  useEffect(() => {
    if (outcome === 'idle') {
      return undefined;
    }
    const timer = window.setTimeout(() => {
      setOutcome('idle');
    }, ANNOUNCEMENT_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [outcome]);

  return (
    <span className={styles.action}>
      <CopyButton value={value} label={label} onCopied={(copied) => setOutcome(copied ? 'copied' : 'failed')} />
      <span role="status" className={styles.status}>
        {outcome === 'copied' ? t('wl.apireference.sample.copied') : null}
        {outcome === 'failed' ? t('wl.apireference.sample.failed') : null}
      </span>
    </span>
  );
}
