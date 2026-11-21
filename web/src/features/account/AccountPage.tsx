import { useTranslation } from 'react-i18next';
import type { User } from '../../api/types';
import { signOut } from '../../app/signOut';
import { nameOf } from './identity';
import styles from './AccountPage.module.css';

export interface AccountPageProps {
  user: User;
  providerName: string | null;
}

export function AccountPage({ user, providerName }: AccountPageProps) {
  const { t } = useTranslation();
  return (
    <section className={styles.page} aria-labelledby="account-title">
      <h1 id="account-title" className={styles.title}>
        {t('wl.account.title')}
      </h1>
      <dl className={styles.facts}>
        <div className={styles.fact}>
          <dt className={styles.term}>{t('wl.account.profile.name')}</dt>
          <dd className={styles.value}>{nameOf(user) ?? t('wl.account.profile.noName')}</dd>
        </div>
        <div className={styles.fact}>
          <dt className={styles.term}>{t('wl.account.profile.email')}</dt>
          <dd className={styles.value}>{user.email}</dd>
        </div>
        {providerName !== null && (
          <div className={styles.fact}>
            <dt className={styles.term}>{t('wl.account.profile.provider')}</dt>
            <dd className={styles.value}>{providerName}</dd>
          </div>
        )}
      </dl>
      <button type="button" className={styles.signOut} onClick={() => void signOut()}>
        {t('wl.account.signOut')}
      </button>
    </section>
  );
}
