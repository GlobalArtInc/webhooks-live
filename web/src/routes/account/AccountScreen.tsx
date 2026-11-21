import { useLocation } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { useInstance } from '../../api/queries';
import { signInHref } from '../../app/signIn';
import { StatePage } from '../../components/StatePage/StatePage';
import { AccountPage } from '../../features/account';
import styles from './AccountScreen.module.css';

export function AccountScreen() {
  const { t } = useTranslation();
  const instance = useInstance().data;
  const location = useLocation();

  if (instance === undefined) {
    return null;
  }
  if (instance.user === null) {
    if (instance.auth_mode === 'open') {
      return (
        <section className={styles.page}>
          <h1 className={styles.title}>{t('wl.account.unavailable.title')}</h1>
          <p className={styles.body}>{t('wl.account.unavailable.body')}</p>
          <a className={styles.home} href="/">
            {t('wl.state.notFound.home')}
          </a>
        </section>
      );
    }
    return (
      <StatePage
        kind="signin"
        detail={{ providerName: instance.auth.provider_name, signInHref: signInHref(location.href) }}
      />
    );
  }
  return <AccountPage user={instance.user} providerName={instance.auth.provider_name} />;
}
