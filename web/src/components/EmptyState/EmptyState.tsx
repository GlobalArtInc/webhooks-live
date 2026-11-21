import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { CopyButton } from '../CopyButton/CopyButton';
import styles from './EmptyState.module.css';

export interface EmptyStateProps {
  captureUrl: string;
  curl: string;
}

export function EmptyState({ captureUrl, curl }: EmptyStateProps) {
  const { t } = useTranslation();
  const titleId = useId();
  return (
    <section className={styles.root} aria-labelledby={titleId}>
      <h2 id={titleId} className={styles.title}>
        <span className={styles.signal} aria-hidden="true" />
        {t('wl.endpoint.empty.title')}
      </h2>
      <p className={styles.url}>
        <code>{captureUrl}</code>
      </p>
      <div className={styles.copyRow}>
        <CopyButton value={captureUrl} label={t('wl.endpoint.header.copyUrl')} />
      </div>
      <p className={styles.hint}>{t('wl.endpoint.empty.hint')}</p>
      <div className={styles.example}>
        <div className={styles.exampleHeader}>
          <span className={styles.exampleLabel}>{t('wl.endpoint.empty.curlLabel')}</span>
          <CopyButton value={curl} label={t('wl.endpoint.header.copyCurl')} />
        </div>
        <pre className={styles.code} tabIndex={0}>
          <code>{curl}</code>
        </pre>
      </div>
    </section>
  );
}
