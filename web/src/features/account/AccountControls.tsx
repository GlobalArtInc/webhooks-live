import { Link, useLocation } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import type { Instance } from '../../api/types';
import { signInHref } from '../../app/signIn';
import { AccountMenu } from './AccountMenu';
import styles from './AccountControls.module.css';

export interface AccountControlsProps {
  instance: Instance;
}

export function AccountControls({ instance }: AccountControlsProps) {
  const { t } = useTranslation();
  const location = useLocation();
  if (instance.user !== null) {
    return <AccountMenu user={instance.user} />;
  }
  const canSignIn = instance.auth_mode !== 'open' && instance.auth.sign_in_available;
  return (
    <nav className={styles.links} aria-label={t('wl.account.menu.label')}>
      {canSignIn && (
        <a className={styles.link} href={signInHref(location.href)}>
          {t('wl.account.signIn')}
        </a>
      )}
      <Link to="/settings" className={styles.link} activeProps={{ 'aria-current': 'page' }}>
        {t('wl.account.preferences')}
      </Link>
    </nav>
  );
}
