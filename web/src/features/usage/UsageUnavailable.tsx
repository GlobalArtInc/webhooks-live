import { useTranslation } from 'react-i18next';
import styles from './UsageUnavailable.module.css';

export function UsageUnavailable() {
  const { t } = useTranslation();
  return (
    <section className={styles.page}>
      <h1 className={styles.title}>{t('wl.usage.unavailable.title')}</h1>
      <p className={styles.body}>{t('wl.usage.unavailable.body')}</p>
      <a className={styles.home} href="/">
        {t('wl.state.notFound.home')}
      </a>
    </section>
  );
}
