import { useTranslation } from 'react-i18next';
import styles from './NewPill.module.css';

export interface NewPillProps {
  count: number;
  onActivate: () => void;
}

export function NewPill({ count, onActivate }: NewPillProps) {
  const { t } = useTranslation();
  if (count <= 0) {
    return null;
  }
  return (
    <button type="button" className={styles.pill} aria-label={t('wl.live.pill.hint', { count })} onClick={onActivate}>
      <span aria-hidden="true">↑</span>
      <span aria-hidden="true">{t('wl.live.pill.label', { count })}</span>
    </button>
  );
}
