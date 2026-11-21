import { useTranslation } from 'react-i18next';
import { FollowToggle } from '../../components/FollowToggle/FollowToggle';
import type { FollowState } from './follow';
import styles from './LiveToolbar.module.css';

export interface LiveToolbarProps {
  follow: FollowState;
  announce: boolean;
  onToggleFollow: () => void;
  onToggleAnnounce: () => void;
}

export function LiveToolbar({ follow, announce, onToggleFollow, onToggleAnnounce }: LiveToolbarProps) {
  const { t } = useTranslation();
  return (
    <div role="group" aria-label={t('wl.live.toolbar.label')} className={styles.toolbar}>
      <FollowToggle enabled={follow.enabled} paused={follow.paused} onToggle={onToggleFollow} />
      <button type="button" className={styles.button} aria-pressed={announce} onClick={onToggleAnnounce}>
        {t('wl.live.announce.toggle')}
      </button>
    </div>
  );
}
