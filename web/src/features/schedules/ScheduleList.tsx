import { useTranslation } from 'react-i18next';
import { formatElapsedTime } from '../../components/format/format';
import type { Schedule } from '../../api/types';
import { cronToCadence } from './cadence';
import { formatRunTime } from './cron';
import styles from './ScheduleList.module.css';

export interface ScheduleListProps {
  schedules: readonly Schedule[];
  selectedId: string | null;
  now: number;
  onSelect: (scheduleId: string) => void;
}

export function ScheduleList({ schedules, selectedId, now, onSelect }: ScheduleListProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  return (
    <ul className={styles.list} aria-label={t('wl.schedules.list.label')}>
      {schedules.map((schedule) => {
        const selected = schedule.id === selectedId;
        const cadence = cronToCadence(schedule.cron);
        return (
          <li key={schedule.id}>
            <button
              type="button"
              className={selected ? `${styles.item} ${styles.selected}` : styles.item}
              aria-current={selected ? 'true' : undefined}
              onClick={() => {
                onSelect(schedule.id);
              }}
            >
              <span className={styles.head}>
                <span className={styles.name}>{schedule.name}</span>
                <span className={schedule.enabled ? styles.active : styles.paused}>
                  {schedule.enabled ? t('wl.schedules.status.active') : t('wl.schedules.status.paused')}
                </span>
              </span>
              <span className={styles.meta}>
                {cadence.kind === 'interval' && cadence.unit === 'minutes'
                  ? t('wl.schedules.summary.everyMinutes', { count: cadence.every })
                  : null}
                {cadence.kind === 'interval' && cadence.unit === 'hours'
                  ? t('wl.schedules.summary.everyHours', { count: cadence.every })
                  : null}
                {cadence.kind === 'daily'
                  ? t('wl.schedules.summary.daily', { time: cadence.time, timezone: schedule.timezone })
                  : null}
                {cadence.kind === 'expression' ? `${cadence.expression} ${schedule.timezone}` : null}
              </span>
              <span className={styles.meta}>
                {schedule.enabled && schedule.next_run_at !== null
                  ? t('wl.schedules.nextRun', {
                      time: formatRunTime(Date.parse(schedule.next_run_at), locale, schedule.timezone),
                    })
                  : t('wl.schedules.noNextRun')}
              </span>
              <span className={styles.meta}>
                {schedule.last_run_at === null
                  ? t('wl.schedules.neverRun')
                  : t('wl.schedules.lastRun', { time: formatElapsedTime(schedule.last_run_at, now, locale) })}
              </span>
              {schedule.consecutive_failures > 0 ? (
                <span className={styles.failures}>
                  {t('wl.schedules.failures', { count: schedule.consecutive_failures })}
                </span>
              ) : null}
            </button>
          </li>
        );
      })}
    </ul>
  );
}
