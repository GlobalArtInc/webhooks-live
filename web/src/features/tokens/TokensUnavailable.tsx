import { useTranslation } from 'react-i18next';
import styles from './TokensUnavailable.module.css';

export function TokensUnavailable() {
  const { t } = useTranslation();
  return (
    <section className={styles.page}>
      <h1 className={styles.title}>{t('wl.tokens.unavailable.title')}</h1>
      <p className={styles.body}>{t('wl.tokens.unavailable.body')}</p>
      <a className={styles.home} href="/">
        {t('wl.state.notFound.home')}
      </a>
    </section>
  );
}
