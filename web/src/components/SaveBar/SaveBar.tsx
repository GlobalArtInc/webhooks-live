import { useTranslation } from 'react-i18next';
import styles from './SaveBar.module.css';

export interface SaveBarProps {
  dirty: boolean;
  saving: boolean;
  failed: boolean;
  onSave: () => void;
  onDiscard: () => void;
}

export function SaveBar({ dirty, saving, failed, onSave, onDiscard }: SaveBarProps) {
  const { t } = useTranslation();
  const idle = !dirty || saving;
  return (
    <div className={styles.bar}>
      <p className={styles.status} role={failed ? 'alert' : undefined}>
        {failed ? t('wl.workspace.saveBar.failed') : dirty ? t('wl.workspace.saveBar.unsaved') : null}
      </p>
      <div className={styles.actions}>
        <button type="button" className={styles.secondary} disabled={idle} onClick={onDiscard}>
          {t('wl.workspace.saveBar.discard')}
        </button>
        <button type="button" className={styles.primary} disabled={idle} onClick={onSave}>
          {saving ? t('wl.workspace.saveBar.saving') : t('wl.workspace.saveBar.save')}
        </button>
      </div>
    </div>
  );
}
