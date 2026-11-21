import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { UsageMeter } from '../../api/usage';
import { formatAbsoluteTime, formatRelativeTime } from '../../components/format/format';
import { fillPercent, meterLevel } from './meterLevel';
import { atLimitText, formatAmount, meterName, windowLabel } from './meterText';
import styles from './UsageMeterItem.module.css';

export interface UsageMeterItemProps {
  meter: UsageMeter;
  now: number;
  locale: string;
}

export function UsageMeterItem({ meter, now, locale }: UsageMeterItemProps) {
  const { t } = useTranslation();
  const nameId = useId();
  const level = meterLevel(meter);
  const valueText = t('wl.usage.value', {
    used: formatAmount(meter, meter.used, locale),
    max: formatAmount(meter, meter.max, locale),
  });
  const sentence = atLimitText(t, meter, level, locale, now);
  const resetsAt = meter.resets_at;

  return (
    <div className={styles.meter} data-level={level}>
      <div className={styles.head}>
        <h2 id={nameId} className={styles.name}>
          {meterName(t, meter.name)}
        </h2>
        <p className={styles.value}>{valueText}</p>
      </div>
      <div
        role="progressbar"
        aria-labelledby={nameId}
        aria-valuemin={0}
        aria-valuemax={meter.max}
        aria-valuenow={Math.min(meter.used, meter.max)}
        aria-valuetext={valueText}
        className={styles.track}
      >
        <span className={styles.fill} style={{ width: `${fillPercent(meter)}%` }} />
      </div>
      <p className={styles.window}>
        <span>{windowLabel(t, meter.window)}</span>
        {resetsAt !== null && (
          <span>
            {t('wl.usage.resets', { when: formatRelativeTime(resetsAt, now, locale) })}{' '}
            <time dateTime={resetsAt}>{formatAbsoluteTime(resetsAt, locale)}</time>
          </span>
        )}
      </p>
      {sentence !== null && <p className={styles.sentence}>{sentence}</p>}
    </div>
  );
}
