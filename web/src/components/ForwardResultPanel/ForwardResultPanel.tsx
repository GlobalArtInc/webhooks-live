import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { ForwardAttempt, ForwardJob } from '../../api/types';
import { formatAbsoluteTime } from '../format/format';
import { StatusChip } from '../StatusChip/StatusChip';
import styles from './ForwardResultPanel.module.css';

export interface ForwardResultPanelProps {
  results: readonly ForwardJob[];
  onResend: () => void;
}

interface ForwardRow {
  key: string;
  job: ForwardJob;
  attempt: ForwardAttempt | null;
}

const EMPTY_CELL = '—';

function rowsOf(results: readonly ForwardJob[]): ForwardRow[] {
  return results.flatMap((job): ForwardRow[] =>
    job.attempts.length === 0
      ? [{ key: job.id, job, attempt: null }]
      : job.attempts.map((attempt) => ({ key: `${job.id}-${attempt.attempt}`, job, attempt })),
  );
}

export function ForwardResultPanel({ results, onResend }: ForwardResultPanelProps) {
  const { t, i18n } = useTranslation();
  const labels = {
    failed: t('wl.detail.forward.failed'),
    pending: t('wl.detail.forward.pending'),
    running: t('wl.detail.forward.running'),
  };

  const statusOf = (row: ForwardRow): ReactNode => {
    if (row.attempt !== null && row.attempt.status !== null) {
      return <StatusChip code={row.attempt.status} kind="forward" label={String(row.attempt.status)} />;
    }
    if (row.job.state === 'failed' || (row.attempt !== null && row.attempt.error !== null)) {
      return <span className={styles.failed}>{labels.failed}</span>;
    }
    if (row.job.state === 'running') {
      return <span className={styles.pending}>{labels.running}</span>;
    }
    return <span className={styles.pending}>{labels.pending}</span>;
  };

  return (
    <div className={styles.panel}>
      <div className={styles.toolbar}>
        <button type="button" className={styles.resend} onClick={onResend}>
          {t('wl.detail.forward.resend')}
        </button>
      </div>
      <div className={styles.scroller}>
        <table aria-label={t('wl.detail.section.forward')} className={styles.table}>
          <thead>
            <tr>
              <th scope="col">{t('wl.detail.forward.target')}</th>
              <th scope="col">{t('wl.detail.forward.status')}</th>
              <th scope="col">{t('wl.detail.forward.duration')}</th>
              <th scope="col">{t('wl.detail.forward.error')}</th>
              <th scope="col">{t('wl.detail.forward.time')}</th>
            </tr>
          </thead>
          <tbody>
            {rowsOf(results).map((row) => {
              const at = row.attempt?.started_at ?? row.job.created_at;
              return (
                <tr key={row.key}>
                  <td className={styles.target}>{row.job.target_url}</td>
                  <td>{statusOf(row)}</td>
                  <td className={styles.meta}>
                    {row.attempt === null
                      ? EMPTY_CELL
                      : t('wl.detail.forward.durationValue', { milliseconds: row.attempt.duration_ms })}
                  </td>
                  <td className={styles.error}>{row.attempt?.error ?? EMPTY_CELL}</td>
                  <td className={styles.meta}>
                    <time dateTime={at}>{formatAbsoluteTime(at, i18n.language)}</time>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
