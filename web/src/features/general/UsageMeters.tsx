import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { offersUsage } from '../../api/capabilities';
import { useInstance } from '../../api/queries';
import { useUsage } from '../../api/usage';
import { formatSize } from '../body/bytes';
import { formatDate } from '../tokens/time';
import { meterState, usageLabelKey, type MeterState } from './model';
import styles from './UsageMeters.module.css';

const PERCENT = 100;

type WindowKey = 'wl.general.usage.window.hour' | 'wl.general.usage.window.day' | 'wl.general.usage.window.total';

function windowKey(window: string | null): WindowKey | null {
  if (window === 'hour') {
    return 'wl.general.usage.window.hour';
  }
  if (window === 'day') {
    return 'wl.general.usage.window.day';
  }
  if (window === 'total') {
    return 'wl.general.usage.window.total';
  }
  return null;
}

export interface UsageMetersProps {
  captureCount: number;
  maxCaptures: number;
  bytesUsed: number;
}

interface MeterRowProps {
  label: string;
  used: number;
  max: number | null;
  detail: string | null;
}

function MeterRow({ label, used, max, detail }: MeterRowProps) {
  const { t } = useTranslation();
  const state: MeterState = meterState(used, max);
  const valueText =
    max === null ? t('wl.general.usage.unlimited', { used }) : t('wl.general.usage.value', { used, max });
  const percent = max === null || max === 0 ? 0 : Math.min(PERCENT, (used / max) * PERCENT);
  const stateText =
    state === 'near' ? t('wl.general.usage.state.near') : state === 'full' ? t('wl.general.usage.state.full') : null;
  const helpText = [stateText, detail].filter((part): part is string => part !== null).join(', ');
  return (
    <div className={styles.meter} data-state={state}>
      <p className={styles.line}>
        <span className={styles.label}>{label}</span>
        <span className={styles.value}>{valueText}</span>
      </p>
      {max === null ? null : (
        <div
          role="progressbar"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={max}
          aria-valuenow={used}
          aria-valuetext={valueText}
          className={styles.track}
        >
          <span className={styles.fill} style={{ width: `${percent}%` }} />
        </div>
      )}
      {helpText === '' ? null : <p className={styles.help}>{helpText}</p>}
    </div>
  );
}

export function UsageMeters({ captureCount, maxCaptures, bytesUsed }: UsageMetersProps) {
  const { t, i18n } = useTranslation();
  const headingId = useId();
  const instance = useInstance().data;
  const offered = offersUsage(instance);
  const usage = useUsage(offered);
  const meters = usage.data?.items ?? [];

  return (
    <section className={styles.group} aria-labelledby={headingId}>
      <h3 id={headingId} className={styles.heading}>
        {t('wl.general.usage.title')}
      </h3>
      <p className={styles.sub}>{t('wl.general.usage.endpoint')}</p>
      <MeterRow label={t('wl.general.usage.captures')} used={captureCount} max={maxCaptures} detail={null} />
      <p className={styles.line}>
        <span className={styles.label}>{t('wl.general.usage.storage')}</span>
        <span className={styles.value}>{formatSize(bytesUsed)}</span>
      </p>
      <p className={styles.sub}>{t('wl.general.usage.account')}</p>
      {instance !== undefined && !offered ? (
        <p className={styles.help}>{t('wl.general.usage.accountUnavailable')}</p>
      ) : null}
      {usage.isError ? (
        <p role="alert" className={styles.error}>
          {t('wl.general.usage.failed')}
        </p>
      ) : null}
      {meters.map((meter) => {
        const labelKey = usageLabelKey(meter.name);
        const windowLabelKey = windowKey(meter.window);
        const parts = [
          windowLabelKey === null ? null : t(windowLabelKey),
          meter.resets_at === null
            ? null
            : t('wl.general.usage.resets', { time: formatDate(meter.resets_at, i18n.language) }),
        ].filter((part): part is string => part !== null);
        return (
          <MeterRow
            key={meter.name}
            label={labelKey === null ? meter.name : t(labelKey)}
            used={meter.used}
            max={meter.max}
            detail={parts.length === 0 ? null : parts.join(', ')}
          />
        );
      })}
      <p className={styles.help}>{t('wl.general.usage.footer')}</p>
    </section>
  );
}
