import { useTranslation } from 'react-i18next';
import styles from './ListPager.module.css';

export interface ListPagerProps {
  position: number | null;
  total: number | null;
  onFirst: () => void;
  onPrev: () => void;
  onNext: () => void;
  onLast: () => void;
}

export function ListPager({ position, total, onFirst, onPrev, onNext, onLast }: ListPagerProps) {
  const { t } = useTranslation();
  const known = position !== null;
  const atStart = !known || position <= 1;
  const atEnd = !known || total === null || position >= total;

  return (
    <nav className={styles.pager} aria-label={t('wl.endpoint.pager.label')}>
      <button type="button" className={styles.button} onClick={onFirst} disabled={atStart}>
        {t('wl.endpoint.pager.first')}
      </button>
      <button type="button" className={styles.button} onClick={onPrev} disabled={atStart}>
        {t('wl.endpoint.pager.previous')}
      </button>
      <span className={styles.position}>
        {known ? t('wl.endpoint.pager.position', { position, total: total ?? position }) : null}
      </span>
      <button type="button" className={styles.button} onClick={onNext} disabled={atEnd}>
        {t('wl.endpoint.pager.next')}
      </button>
      <button type="button" className={styles.button} onClick={onLast} disabled={atEnd}>
        {t('wl.endpoint.pager.last')}
      </button>
    </nav>
  );
}
