import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './TrySteps.module.css';

export function TrySteps() {
  const { t } = useTranslation();
  const headingId = useId();

  return (
    <section aria-labelledby={headingId} className={styles.steps}>
      <h2 id={headingId} className={styles.heading}>
        {t('wl.home.steps.heading')}
      </h2>
      <ol className={styles.list}>
        <li className={styles.card}>
          <span className={styles.label}>{t('wl.home.steps.create.label')}</span>
          <span className={styles.body}>{t('wl.home.steps.create.body')}</span>
        </li>
        <li className={styles.card}>
          <span className={styles.label}>{t('wl.home.steps.send.label')}</span>
          <span className={styles.body}>{t('wl.home.steps.send.body')}</span>
        </li>
        <li className={styles.card}>
          <span className={styles.label}>{t('wl.home.steps.watch.label')}</span>
          <span className={styles.body}>{t('wl.home.steps.watch.body')}</span>
        </li>
      </ol>
    </section>
  );
}
