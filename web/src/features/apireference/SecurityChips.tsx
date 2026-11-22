import { useTranslation } from 'react-i18next';
import type { SecurityChip } from './reference';
import styles from './SecurityChips.module.css';

export interface SecurityChipsProps {
  chips: readonly SecurityChip[];
}

export function SecurityChips({ chips }: SecurityChipsProps) {
  const { t } = useTranslation();
  return (
    <ul className={styles.chips}>
      {chips.map((chip) => (
        <li key={`${chip.kind}:${chip.name}`} className={styles.chip} data-kind={chip.kind}>
          {chip.kind === 'session' ? t('wl.apireference.security.session') : null}
          {chip.kind === 'token' ? t('wl.apireference.security.token') : null}
          {chip.kind === 'public' ? t('wl.apireference.security.public') : null}
          {chip.kind === 'other' ? chip.name : null}
        </li>
      ))}
    </ul>
  );
}
