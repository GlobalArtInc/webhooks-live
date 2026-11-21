import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { formatDate } from '../tokens/time';
import {
  captureTtlOptions,
  isCaptureTtlValid,
  isLifetimeValid,
  isMaxCapturesValid,
  lifetimeOptions,
  type DurationOption,
  type GeneralDraft,
  type GeneralLimits,
} from './model';
import styles from './LifetimeField.module.css';

const DAY_SECONDS = 86400;
const HOUR_SECONDS = 3600;
const OFF_VALUE = 'off';

export interface LifetimeFieldProps {
  draft: GeneralDraft;
  limits: GeneralLimits;
  readOnly: boolean;
  expiresAt: string | null;
  onChange: (draft: GeneralDraft) => void;
}

function optionValue(option: DurationOption): string {
  return option.seconds === null ? OFF_VALUE : String(option.seconds);
}

export function LifetimeField({ draft, limits, readOnly, expiresAt, onChange }: LifetimeFieldProps) {
  const { t, i18n } = useTranslation();
  const baseId = useId();
  const modeName = `${baseId}-mode`;
  const windowId = `${baseId}-window`;
  const ttlId = `${baseId}-ttl`;
  const maxId = `${baseId}-max`;
  const maxErrorId = `${maxId}-error`;
  const windowErrorId = `${windowId}-error`;
  const sliding = draft.retentionMode === 'sliding';
  const seconds = draft.retentionSeconds ?? limits.defaultRetentionSeconds;
  const windowOptions = lifetimeOptions(limits.maxRetentionSeconds, seconds);
  const selected = windowOptions.find((option) => option.seconds === seconds);
  const selectedLabel = selected?.label ?? 'wl.general.lifetime.preset.custom';
  const windowValid = isLifetimeValid(draft, limits);
  const maxValid = isMaxCapturesValid(draft.maxCaptures, limits);
  const neverBlocked = !limits.neverAllowed && !sliding;
  const maxDays = limits.maxRetentionSeconds / DAY_SECONDS;
  const ttlValid = isCaptureTtlValid(draft.captureTtl);
  const ttlOptions = captureTtlOptions(draft.captureTtl);

  return (
    <section className={styles.group} aria-labelledby={`${baseId}-heading`}>
      <h3 id={`${baseId}-heading`} className={styles.heading}>
        {t('wl.general.lifetime.title')}
      </h3>
      {readOnly ? <p className={styles.help}>{t('wl.general.lifetime.ownerOnly')}</p> : null}
      <fieldset className={styles.fieldset} disabled={readOnly}>
        <legend className={styles.legend}>{t('wl.general.lifetime.mode.legend')}</legend>
        <label className={styles.choice}>
          <input
            type="radio"
            name={modeName}
            value="sliding"
            checked={sliding}
            onChange={() => {
              onChange({ ...draft, retentionMode: 'sliding' });
            }}
          />
          {t('wl.general.lifetime.mode.sliding')}
        </label>
        <div className={styles.indent}>
          <label htmlFor={windowId} className={styles.label}>
            {t('wl.general.lifetime.window.label')}
          </label>
          <select
            id={windowId}
            className={styles.input}
            value={String(seconds)}
            disabled={!sliding}
            aria-invalid={sliding && !windowValid ? true : undefined}
            aria-describedby={sliding && !windowValid ? windowErrorId : undefined}
            onChange={(event) => {
              onChange({ ...draft, retentionSeconds: Number(event.target.value) });
            }}
          >
            {windowOptions.map((option) => (
              <option key={optionValue(option)} value={optionValue(option)}>
                {t(option.label, { seconds: option.seconds ?? 0 })}
              </option>
            ))}
          </select>
          {sliding ? (
            <p className={styles.help}>
              {t('wl.general.lifetime.outcome.sliding', { window: t(selectedLabel, { seconds }) })}
            </p>
          ) : null}
          {sliding ? (
            <p className={styles.help}>
              {Number.isInteger(maxDays)
                ? t('wl.general.lifetime.limit.days', { count: maxDays })
                : t('wl.general.lifetime.limit.hours', {
                    count: Math.round(limits.maxRetentionSeconds / HOUR_SECONDS),
                  })}
            </p>
          ) : null}
          {sliding && expiresAt !== null ? (
            <p className={styles.help}>
              {t('wl.general.lifetime.next', { date: formatDate(expiresAt, i18n.language) })}
            </p>
          ) : null}
          {sliding && !windowValid ? (
            <p id={windowErrorId} role="alert" className={styles.error}>
              {t('wl.general.lifetime.invalid')}
            </p>
          ) : null}
        </div>
        <label className={styles.choice}>
          <input
            type="radio"
            name={modeName}
            value="never"
            checked={!sliding}
            disabled={neverBlocked}
            onChange={() => {
              onChange({ ...draft, retentionMode: 'never' });
            }}
          />
          {t('wl.general.lifetime.mode.never')}
        </label>
        <div className={styles.indent}>
          {draft.retentionMode === 'never' ? (
            <p className={styles.help}>{t('wl.general.lifetime.outcome.never')}</p>
          ) : null}
          {neverBlocked ? <p className={styles.help}>{t('wl.general.lifetime.outcome.neverBlocked')}</p> : null}
        </div>
        <div className={styles.field}>
          <label htmlFor={ttlId} className={styles.label}>
            {t('wl.general.lifetime.captureTtl.label')}
          </label>
          <select
            id={ttlId}
            className={styles.input}
            value={draft.captureTtl === null ? OFF_VALUE : String(draft.captureTtl)}
            aria-invalid={ttlValid ? undefined : true}
            onChange={(event) => {
              onChange({ ...draft, captureTtl: event.target.value === OFF_VALUE ? null : Number(event.target.value) });
            }}
          >
            {ttlOptions.map((option) => (
              <option key={optionValue(option)} value={optionValue(option)}>
                {t(option.label, { seconds: option.seconds ?? 0 })}
              </option>
            ))}
          </select>
          <p className={styles.help}>{t('wl.general.lifetime.captureTtl.help')}</p>
        </div>
        <div className={styles.field}>
          <label htmlFor={maxId} className={styles.label}>
            {t('wl.general.lifetime.maxCaptures.label')}
          </label>
          <input
            id={maxId}
            type="text"
            inputMode="numeric"
            autoComplete="off"
            className={styles.input}
            value={draft.maxCaptures}
            aria-invalid={maxValid ? undefined : true}
            aria-describedby={maxValid ? undefined : maxErrorId}
            onChange={(event) => {
              onChange({ ...draft, maxCaptures: event.target.value });
            }}
          />
          <p className={styles.help}>{t('wl.general.lifetime.maxCaptures.help')}</p>
          <p className={styles.help}>{t('wl.general.lifetime.maxCaptures.limit', { count: limits.maxCaptures })}</p>
          {maxValid ? null : (
            <p id={maxErrorId} role="alert" className={styles.error}>
              {t('wl.general.lifetime.maxCaptures.invalid')}
            </p>
          )}
        </div>
      </fieldset>
    </section>
  );
}
