import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import type { CaptureSummary } from '../../api/types';
import { formatRelativeTime } from '../format/format';
import styles from './CapturePicker.module.css';

export interface LatestPick {
  mode: 'latest';
}

export interface PinnedPick {
  mode: 'pinned';
  captureId: string;
}

export type CapturePick = LatestPick | PinnedPick;

export interface CapturePickerProps {
  captures: readonly CaptureSummary[] | undefined;
  value: CapturePick;
  onChange: (value: CapturePick) => void;
  onSendSample: () => void;
  now: number;
  sendingSample?: boolean;
  disabled?: boolean;
}

const LATEST_VALUE = 'latest';

export function CapturePicker({
  captures,
  value,
  onChange,
  onSendSample,
  now,
  sendingSample = false,
  disabled = false,
}: CapturePickerProps) {
  const { t, i18n } = useTranslation();
  const selectId = useId();
  const statusId = useId();
  const locale = i18n.language;
  const loading = captures === undefined;
  const list = captures ?? [];
  const latest = list[0];
  const pinnedId = value.mode === 'pinned' ? value.captureId : null;
  const pinnedMissing = pinnedId !== null && !list.some((capture) => capture.id === pinnedId);

  function describe(capture: CaptureSummary): string {
    return t('wl.picker.option', {
      method: capture.method,
      path: capture.path,
      time: formatRelativeTime(capture.received_at, now, locale),
    });
  }

  function handleSelect(next: string): void {
    onChange(next === LATEST_VALUE ? { mode: 'latest' } : { mode: 'pinned', captureId: next });
  }

  function togglePin(): void {
    if (value.mode === 'pinned') {
      onChange({ mode: 'latest' });
    } else if (latest !== undefined) {
      onChange({ mode: 'pinned', captureId: latest.id });
    }
  }

  if (!loading && list.length === 0) {
    return (
      <div className={styles.picker} data-state="empty">
        <p className={styles.empty}>{t('wl.picker.empty')}</p>
        <button type="button" className={styles.send} disabled={disabled || sendingSample} onClick={onSendSample}>
          {sendingSample ? t('wl.picker.sending') : t('wl.picker.sendSample')}
        </button>
      </div>
    );
  }

  return (
    <div className={styles.picker} data-state={loading ? 'loading' : value.mode}>
      <div className={styles.controls}>
        <label className={styles.label} htmlFor={selectId}>
          {t('wl.picker.label')}
        </label>
        <select
          id={selectId}
          className={styles.select}
          aria-describedby={statusId}
          disabled={disabled || loading}
          value={pinnedId ?? LATEST_VALUE}
          onChange={(event) => {
            handleSelect(event.target.value);
          }}
        >
          <option value={LATEST_VALUE}>{t('wl.picker.latest')}</option>
          {pinnedMissing && pinnedId !== null ? <option value={pinnedId}>{pinnedId}</option> : null}
          {list.map((capture) => (
            <option key={capture.id} value={capture.id}>
              {describe(capture)}
            </option>
          ))}
        </select>
        <button
          type="button"
          className={styles.pin}
          aria-pressed={value.mode === 'pinned'}
          disabled={disabled || loading || (value.mode === 'latest' && latest === undefined)}
          onClick={togglePin}
        >
          {value.mode === 'pinned' ? t('wl.picker.unpin') : t('wl.picker.pin')}
        </button>
      </div>
      <p id={statusId} className={styles.status} role="status">
        {loading ? t('wl.picker.loading') : value.mode === 'pinned' ? t('wl.picker.pinned') : t('wl.picker.following')}
        {!loading && value.mode === 'latest' && latest !== undefined ? `: ${describe(latest)}` : ''}
      </p>
    </div>
  );
}
