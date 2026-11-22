import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { Capture } from '../../api/types';
import { CopyButton } from '../../components/CopyButton/CopyButton';
import { MethodBadge } from '../../components/MethodBadge/MethodBadge';
import { StatusChip } from '../../components/StatusChip/StatusChip';
import styles from './CaptureDetail.module.css';
import { formatAbsoluteTime, formatByteSize } from '../../components/format/format';
import { useTimeDisplay } from '../../prefs/display';
import { RelativeTime } from './RelativeTime';

interface SummarySectionProps {
  capture: Capture;
}

interface FactProps {
  term: string;
  children: ReactNode;
}

function Fact({ term, children }: FactProps) {
  return (
    <div className={styles.fact}>
      <dt className={styles.factTerm}>{term}</dt>
      <dd className={styles.factValue}>{children}</dd>
    </div>
  );
}

export function SummarySection({ capture }: SummarySectionProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const timeDisplay = useTimeDisplay();
  return (
    <div className={styles.summary}>
      <div className={styles.urlRow}>
        <MethodBadge method={capture.method} />
        <code className={styles.url}>{capture.url}</code>
        <CopyButton
          value={capture.url}
          label={t('wl.detail.summary.copyUrl')}
          idleText={t('wl.detail.summary.copyUrl')}
        />
      </div>
      <dl className={styles.facts}>
        <Fact term={t('wl.detail.summary.clientIp')}>
          <code>{capture.client_ip}</code>
        </Fact>
        <Fact term={t('wl.detail.summary.received')}>
          <time dateTime={capture.received_at}>
            {formatAbsoluteTime(capture.received_at, locale, timeDisplay === 'utc')}
          </time>
          {timeDisplay === 'relative' && (
            <>
              {' '}
              <span className={styles.muted}>
                <RelativeTime iso={capture.received_at} locale={locale} />
              </span>
            </>
          )}
        </Fact>
        <Fact term={t('wl.detail.summary.size')}>{formatByteSize(capture.body_size, locale)}</Fact>
        <Fact term={t('wl.detail.summary.contentType')}>{capture.content_type ?? t('wl.detail.none')}</Fact>
        <Fact term={t('wl.detail.summary.captureId')}>
          <code>{capture.id}</code>
        </Fact>
        <Fact term={t('wl.detail.summary.answered')}>
          <span className={styles.chips}>
            <StatusChip code={capture.response_status} kind="answered" label={String(capture.response_status)} />
            {capture.status_override === null ? null : (
              <StatusChip
                code={capture.status_override}
                kind="override"
                label={t('wl.detail.summary.override', { code: capture.status_override })}
              />
            )}
          </span>
        </Fact>
      </dl>
    </div>
  );
}
