import { useTranslation } from 'react-i18next';
import { useOnlineStatus } from './useOnlineStatus';
import styles from './OfflineBanner.module.css';

export function OfflineBanner() {
  const { t } = useTranslation();
  const online = useOnlineStatus();

  return <div role="status">{!online && <p className={styles.banner}>{t('wl.offline.message')}</p>}</div>;
}
