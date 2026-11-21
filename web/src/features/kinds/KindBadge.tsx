import { useTranslation } from 'react-i18next';
import type { BadgeKind } from './kinds';
import styles from './KindBadge.module.css';

export interface KindBadgeProps {
  kind: BadgeKind;
}

export function KindBadge({ kind }: KindBadgeProps) {
  const { t } = useTranslation();
  const text = kind === 'email' ? t('wl.kinds.badge.email') : t('wl.kinds.badge.dns');
  return (
    <span className={styles.badge} data-kind={kind}>
      {text}
    </span>
  );
}
