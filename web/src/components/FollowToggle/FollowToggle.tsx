import { useTranslation } from 'react-i18next';
import styles from './FollowToggle.module.css';

export interface FollowToggleProps {
  enabled: boolean;
  paused: boolean;
  onToggle: () => void;
}

export function FollowToggle({ enabled, paused, onToggle }: FollowToggleProps) {
  const { t } = useTranslation();
  const stateName = enabled ? (paused ? 'paused' : 'on') : 'off';
  return (
    <button
      type="button"
      className={styles.toggle}
      aria-pressed={enabled && !paused}
      data-state={stateName}
      title={paused ? t('wl.live.follow.pausedHint') : t('wl.live.follow.hint')}
      onClick={onToggle}
    >
      <span className={styles.dot} aria-hidden="true" />
      <span>{t('wl.live.follow.label')}</span>
      {paused ? (
        <>
          {' '}
          <span className={styles.note}>{t('wl.live.follow.paused')}</span>
        </>
      ) : null}
    </button>
  );
}
