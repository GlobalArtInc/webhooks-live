import { Link } from '@tanstack/react-router';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useScheduleRuns } from '../../api/queries';
import type { ScheduleRun } from '../../api/types';
import { formatAbsoluteTime } from '../../components/format/format';
import styles from './ScheduleHistory.module.css';

export interface ScheduleHistoryProps {
  endpointId: string;
  scheduleId: string;
}

const SUCCESS_BELOW = 400;
const MS_PER_SECOND = 1000;

export function isFailedRun(run: ScheduleRun): boolean {
  return run.status === null || run.status >= SUCCESS_BELOW || run.error !== null;
}

export function ScheduleHistory({ endpointId, scheduleId }: ScheduleHistoryProps) {
  const { t, i18n } = useTranslation();
  const baseId = useId();
  const [failedOnly, setFailedOnly] = useState(false);
  const runs = useScheduleRuns(endpointId, scheduleId);
  const items = runs.data?.items ?? [];
  const visible = failedOnly ? items.filter(isFailedRun) : items;
  const headingId = `${baseId}-heading`;
  const filterId = `${baseId}-failed`;

  function durationText(durationMs: number | null): string {
    if (durationMs === null) {
      return '';
    }
    if (durationMs < MS_PER_SECOND) {
      return t('wl.schedules.history.ms', { count: durationMs });
    }
    return t('wl.schedules.history.seconds', { count: Number((durationMs / MS_PER_SECOND).toFixed(1)) });
  }

  return (
    <section aria-labelledby={headingId} className={styles.section}>
      <div className={styles.header}>
        <h3 id={headingId} className={styles.heading}>
          {t('wl.schedules.history.heading')}
        </h3>
        <div className={styles.check}>
          <input
            id={filterId}
            type="checkbox"
            checked={failedOnly}
            onChange={(event) => {
              setFailedOnly(event.target.checked);
            }}
          />
          <label htmlFor={filterId}>{t('wl.schedules.history.failedOnly')}</label>
        </div>
      </div>
      {runs.isPending ? (
        <p role="status" className={styles.muted}>
          {t('wl.workspace.loading')}
        </p>
      ) : null}
      {runs.isError ? (
        <p role="alert" className={styles.error}>
          {t('wl.schedules.error.generic')}
        </p>
      ) : null}
      {runs.isSuccess && visible.length === 0 ? (
        <p className={styles.muted}>
          {failedOnly ? t('wl.schedules.history.emptyFailed') : t('wl.schedules.history.empty')}
        </p>
      ) : null}
      {visible.length > 0 ? (
        <div className={styles.scroll}>
          <table className={styles.table} aria-label={t('wl.schedules.history.table')}>
            <thead>
              <tr>
                <th scope="col">{t('wl.schedules.history.time')}</th>
                <th scope="col">{t('wl.schedules.history.status')}</th>
                <th scope="col">{t('wl.schedules.history.duration')}</th>
                <th scope="col">{t('wl.schedules.history.capture')}</th>
              </tr>
            </thead>
            <tbody>
              {visible.map((run) => (
                <tr key={run.id} className={isFailedRun(run) ? styles.failed : undefined}>
                  <td>
                    <time dateTime={run.started_at}>{formatAbsoluteTime(run.started_at, i18n.language)}</time>
                  </td>
                  <td>
                    {run.status === null ? (
                      <span className={styles.muted}>{t('wl.schedules.history.noResponse')}</span>
                    ) : (
                      <span className={styles.status}>{run.status}</span>
                    )}
                    {run.error === null ? null : <span className={styles.errorText}>{run.error}</span>}
                  </td>
                  <td>{durationText(run.duration_ms)}</td>
                  <td>
                    {run.capture_id === null ? (
                      <span className={styles.muted}>{t('wl.schedules.history.noCapture')}</span>
                    ) : (
                      <Link
                        to="/e/$id/c/$captureId"
                        params={{ id: endpointId, captureId: run.capture_id }}
                        className={styles.link}
                      >
                        {t('wl.schedules.history.open')}
                      </Link>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </section>
  );
}
