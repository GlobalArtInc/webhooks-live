import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import type { Instance } from '../../api/types';
import styles from './TokensLink.module.css';

export interface TokensLinkProps {
  instance: Instance;
}

export function showsTokensLink(instance: Instance): boolean {
  return instance.auth_mode !== 'open' && instance.user !== null;
}

export function TokensLink({ instance }: TokensLinkProps) {
  const { t } = useTranslation();
  if (!showsTokensLink(instance)) {
    return null;
  }
  return (
    <Link to="/settings/tokens" className={styles.link} activeProps={{ 'aria-current': 'page' }}>
      {t('wl.tokens.link')}
    </Link>
  );
}
