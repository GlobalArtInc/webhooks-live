import { useTranslation } from 'react-i18next';
import type { Announcement } from '../../features/live/announcer';
import styles from './Announcer.module.css';

export interface AnnouncerProps {
  announcement: Announcement | null;
  muted: boolean;
}

export function Announcer({ announcement, muted }: AnnouncerProps) {
  const { t } = useTranslation();
  return (
    <div role="status" aria-live="polite" className={styles.region}>
      {announcement === null || muted ? null : (
        <span key={announcement.id}>
          {t('wl.live.announce.newCaptures', {
            count: announcement.count,
            method: announcement.method,
            path: announcement.path,
          })}
        </span>
      )}
    </div>
  );
}
