import { useTranslation } from 'react-i18next';
import styles from './UndoToast.module.css';

interface UndoToastProps {
  message: string;
  onUndo: () => void;
  onDismiss: () => void;
}

export function UndoToast({ message, onUndo, onDismiss }: UndoToastProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.toast} role="status">
      <span className={styles.message}>{message}</span>
      <button type="button" className={styles.action} onClick={onUndo}>
        {t('wl.endpoint.undo.action')}
      </button>
      <button type="button" className={styles.dismiss} onClick={onDismiss}>
        {t('wl.endpoint.undo.dismiss')}
      </button>
    </div>
  );
}
