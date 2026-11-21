import { useTranslation } from 'react-i18next';
import styles from './SelectionBar.module.css';

export interface SelectionBarProps {
  active: boolean;
  count: number;
  loadedCount: number;
  onBegin: () => void;
  onEnd: () => void;
  onSelectAll: () => void;
  onExport: () => void;
  onDelete: () => void;
}

export function SelectionBar({
  active,
  count,
  loadedCount,
  onBegin,
  onEnd,
  onSelectAll,
  onExport,
  onDelete,
}: SelectionBarProps) {
  const { t } = useTranslation();
  if (!active) {
    return (
      <div className={styles.bar} role="group" aria-label={t('wl.selection.bar.label')}>
        <button type="button" className={styles.button} aria-pressed={false} onClick={onBegin}>
          {t('wl.selection.toggle')}
        </button>
        <button type="button" className={styles.button} onClick={onExport}>
          {t('wl.selection.export')}
        </button>
      </div>
    );
  }
  return (
    <div className={styles.bar} role="group" aria-label={t('wl.selection.bar.label')}>
      <span className={styles.count} role="status">
        {t('wl.selection.count', { count })}
      </span>
      <button type="button" className={styles.button} disabled={loadedCount === 0} onClick={onSelectAll}>
        {t('wl.selection.selectAll')}
      </button>
      <button type="button" className={styles.button} onClick={onExport}>
        {t('wl.selection.export')}
      </button>
      <button type="button" className={styles.button} disabled={count === 0} onClick={onDelete}>
        {t('wl.selection.delete')}
      </button>
      <button type="button" className={styles.button} onClick={onEnd}>
        {t('wl.selection.done')}
      </button>
    </div>
  );
}
