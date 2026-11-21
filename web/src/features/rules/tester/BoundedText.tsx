import { useTranslation } from 'react-i18next';
import { VIEW_LIMIT_CHARS } from './constants';
import styles from './Tester.module.css';

export interface BoundedTextProps {
  text: string;
  label: string;
  limit?: number;
}

export function BoundedText({ text, label, limit = VIEW_LIMIT_CHARS }: BoundedTextProps) {
  const { t } = useTranslation();
  const clipped = text.length > limit;
  return (
    <>
      <pre className={styles.pre} tabIndex={0} aria-label={label}>
        {clipped ? text.slice(0, limit) : text}
      </pre>
      {clipped ? <p className={styles.note}>{t('wl.tester.view.clipped', { limit })}</p> : null}
    </>
  );
}
