import type { CSSProperties, ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { CaptureSummary } from '../../api/types';
import type { CompareLabel } from '../../features/list/compare';
import { formatByteSize, formatRelativeTime } from '../format/format';
import { MethodBadge } from '../MethodBadge/MethodBadge';
import { StatusChip } from '../StatusChip/StatusChip';
import styles from './CaptureRow.module.css';

export type CaptureRowVariant = 'one-line' | 'two-line';

export interface CaptureRowProps {
  optionId: string;
  summary: CaptureSummary;
  variant: CaptureRowVariant;
  selected: boolean;
  positionInSet: number;
  setSize: number;
  now: number;
  unread?: boolean;
  arriving?: boolean;
  endpoint?: ReactNode;
  style: CSSProperties;
  onSelect: (captureId: string) => void;
}

export interface CaptureRowActionsProps {
  captureId: string;
  compareLabel: CompareLabel;
  style: CSSProperties;
  compare?: boolean;
  deleteDenied?: string;
  onDelete: (captureId: string) => void;
  onCompare: (captureId: string) => void;
}

export function displayPath(summary: CaptureSummary): string {
  const path = summary.path === '' ? '/' : summary.path;
  return summary.query_string === '' ? path : `${path}?${summary.query_string}`;
}

export function CaptureRow({
  optionId,
  summary,
  variant,
  selected,
  positionInSet,
  setSize,
  now,
  unread = false,
  arriving = false,
  endpoint,
  style,
  onSelect,
}: CaptureRowProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const path = displayPath(summary);
  const size = formatByteSize(summary.body_size, locale);
  const time = formatRelativeTime(summary.received_at, now, locale);
  const contentType = summary.content_type ?? t('wl.endpoint.row.noType');

  const forwardText =
    summary.forward_state === 'pending'
      ? t('wl.endpoint.row.forward.pending')
      : summary.forward_state === 'succeeded'
        ? t('wl.endpoint.row.forward.succeeded')
        : t('wl.endpoint.row.forward.failed');

  return (
    <div
      id={optionId}
      role="option"
      aria-selected={selected}
      aria-setsize={setSize}
      aria-posinset={positionInSet}
      className={styles.option}
      style={style}
      data-variant={variant}
      data-selected={selected ? 'true' : undefined}
      data-unread={unread ? 'true' : undefined}
      data-arriving={arriving ? 'true' : undefined}
      onClick={() => {
        onSelect(summary.id);
      }}
    >
      <span className={styles.line}>
        {unread ? (
          <span className={styles.unread}>
            <span className={styles.unreadDot} aria-hidden="true" />
            <span className={styles.visuallyHidden}>{t('wl.live.row.unread')}</span>
          </span>
        ) : null}
        <MethodBadge method={summary.method} />
        <span className={styles.path} title={path}>
          {path}
        </span>
        {endpoint}
        {summary.status_override === null ? null : (
          <span title={t('wl.endpoint.row.overrideTitle', { code: summary.status_override })}>
            <StatusChip code={summary.status_override} kind="override" label={String(summary.status_override)} />
          </span>
        )}
        {summary.forward_state === null ? null : (
          <span className={styles.forward} data-state={summary.forward_state}>
            {forwardText}
          </span>
        )}
        <span className={styles.size}>{size}</span>
        <time className={styles.time} dateTime={summary.received_at}>
          {time}
        </time>
      </span>
      <span className={styles.meta}>
        <span>{contentType}</span>
        <span>{summary.client_ip}</span>
      </span>
    </div>
  );
}

export function CaptureRowActions({
  captureId,
  compareLabel,
  style,
  compare = true,
  deleteDenied,
  onDelete,
  onCompare,
}: CaptureRowActionsProps) {
  const { t } = useTranslation();
  const compareName =
    compareLabel === 'mark'
      ? t('wl.endpoint.compare.mark')
      : compareLabel === 'unmark'
        ? t('wl.endpoint.compare.unmark')
        : t('wl.endpoint.compare.withMarked');

  return (
    <div className={styles.actions} style={style}>
      <button
        type="button"
        className={styles.action}
        disabled={deleteDenied !== undefined}
        title={deleteDenied}
        onClick={() => {
          onDelete(captureId);
        }}
      >
        <span aria-hidden="true">×</span>
        <span className={styles.visuallyHidden}>{t('wl.endpoint.row.delete')}</span>
      </button>
      {compare ? (
        <button
          type="button"
          className={styles.action}
          onClick={() => {
            onCompare(captureId);
          }}
        >
          <span aria-hidden="true">A/B</span>
          <span className={styles.visuallyHidden}>{compareName}</span>
        </button>
      ) : null}
    </div>
  );
}
