import { useTranslation } from 'react-i18next';
import type { ConnectionState } from '../../features/live/types';
import { formatRelativeTime } from '../format/format';
import styles from './ConnectionIndicator.module.css';

export interface ConnectionIndicatorProps {
  state: ConnectionState;
  lastEventAt: number | null;
  now: number;
}

export function ConnectionIndicator({ state, lastEventAt, now }: ConnectionIndicatorProps) {
  const { t, i18n } = useTranslation();
  const tooltip =
    lastEventAt === null
      ? t('wl.live.status.noEvent')
      : t('wl.live.status.lastEvent', {
          when: formatRelativeTime(new Date(lastEventAt).toISOString(), now, i18n.language),
        });

  let label: string;
  switch (state) {
    case 'open':
      label = t('wl.live.status.live');
      break;
    case 'connecting':
      label = t('wl.live.status.connecting');
      break;
    case 'reconnecting':
      label = t('wl.live.status.reconnecting');
      break;
    case 'offline':
      label = t('wl.live.status.offline');
      break;
    case 'ended':
      label = t('wl.live.status.ended');
      break;
  }

  return (
    <span className={styles.root} data-state={state} title={tooltip}>
      <span className={styles.dot} aria-hidden="true" />
      <span className={styles.visuallyHidden}>{t('wl.live.status.label')}: </span>
      <span className={styles.text}>{label}</span>
    </span>
  );
}
