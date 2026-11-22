import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { offersUsage } from '../../api/capabilities';
import type { Instance } from '../../api/types';
import { useUsage } from '../../api/usage';
import { headerWarning } from './meterText';
import styles from './UsageLink.module.css';

export interface UsageLinkProps {
  instance: Instance;
}

export function showsUsageLink(instance: Instance): boolean {
  return offersUsage(instance);
}

export function UsageLink({ instance }: UsageLinkProps) {
  const { t, i18n } = useTranslation();
  const shown = showsUsageLink(instance);
  const usage = useUsage(shown);
  if (!shown) {
    return null;
  }
  const warning = usage.data === undefined ? null : headerWarning(t, usage.data.items, i18n.language);

  return (
    <Link to="/usage" className={styles.link} activeProps={{ 'aria-current': 'page' }}>
      {t('wl.usage.link')}
      {warning !== null && (
        <span className={styles.warning} data-level={warning.level}>
          <span className={styles.dot} aria-hidden="true" />
          {warning.text}
        </span>
      )}
    </Link>
  );
}
