import { useTranslation } from 'react-i18next';
import { useOpenShortcutsHelp } from './hooks';
import styles from './ShortcutsButton.module.css';

export function ShortcutsButton() {
  const { t } = useTranslation();
  const openHelp = useOpenShortcutsHelp();
  return (
    <button type="button" className={styles.button} aria-keyshortcuts="?" onClick={openHelp}>
      {t('wl.shortcuts.open')}
    </button>
  );
}
