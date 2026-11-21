import { useTranslation } from 'react-i18next';
import styles from './ConflictBanner.module.css';

export interface ConflictBannerProps {
  onReload: () => void;
  onOverwrite: () => void;
}

export function ConflictBanner({ onReload, onOverwrite }: ConflictBannerProps) {
  const { t } = useTranslation();
  return (
    <div role="alert" className={styles.banner}>
      <p className={styles.message}>{t('wl.workspace.conflict.message')}</p>
      <div className={styles.actions}>
        <button type="button" className={styles.secondary} onClick={onReload}>
          {t('wl.workspace.conflict.reload')}
        </button>
        <button type="button" className={styles.primary} onClick={onOverwrite}>
          {t('wl.workspace.conflict.overwrite')}
        </button>
      </div>
    </div>
  );
}
