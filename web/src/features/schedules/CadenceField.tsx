import { useId, useMemo, type ChangeEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { cadenceToExpression, listTimezones, type CadenceValue, type IntervalUnit } from './cadence';
import { formatRunTime } from './cron';
import { useScheduleIssueText, type ScheduleTextLimits } from './issueText';
import type { DraftIssue } from './scheduleDraft';
import styles from './CadenceField.module.css';

export interface CadenceFieldProps {
  value: CadenceValue;
  timezone: string;
  nextRuns: readonly number[] | null;
  issue: DraftIssue | null;
  timezoneIssue: DraftIssue | null;
  limits: ScheduleTextLimits;
  readOnly: boolean;
  onChange: (value: CadenceValue) => void;
  onTimezoneChange: (timezone: string) => void;
}

interface IntervalPreset {
  every: number;
  unit: IntervalUnit;
}

type ExpressionPresetId = 'everyFiveMinutes' | 'hourly' | 'dailyNight' | 'weekdayMorning';

interface ExpressionPreset {
  id: ExpressionPresetId;
  expression: string;
}

const INTERVAL_PRESETS: readonly IntervalPreset[] = [
  { every: 5, unit: 'minutes' },
  { every: 15, unit: 'minutes' },
  { every: 30, unit: 'minutes' },
  { every: 1, unit: 'hours' },
  { every: 6, unit: 'hours' },
];

const EXPRESSION_PRESETS: readonly ExpressionPreset[] = [
  { id: 'everyFiveMinutes', expression: '*/5 * * * *' },
  { id: 'hourly', expression: '0 * * * *' },
  { id: 'dailyNight', expression: '0 2 * * *' },
  { id: 'weekdayMorning', expression: '0 9 * * 1-5' },
];

const KINDS = ['interval', 'daily', 'expression'] as const;

function numberText(every: number): string {
  return Number.isNaN(every) ? '' : String(every);
}

function parseNumber(text: string): number {
  return text.trim() === '' ? Number.NaN : Number(text);
}

export function CadenceField({
  value,
  timezone,
  nextRuns,
  issue,
  timezoneIssue,
  limits,
  readOnly,
  onChange,
  onTimezoneChange,
}: CadenceFieldProps) {
  const { t, i18n } = useTranslation();
  const baseId = useId();
  const issueText = useScheduleIssueText(limits);
  const timezones = useMemo(() => listTimezones(timezone), [timezone]);
  const kindName = `${baseId}-kind`;
  const everyId = `${baseId}-every`;
  const unitId = `${baseId}-unit`;
  const timeId = `${baseId}-time`;
  const expressionId = `${baseId}-expression`;
  const timezoneId = `${baseId}-timezone`;
  const issueId = `${baseId}-issue`;
  const timezoneIssueId = `${baseId}-timezone-issue`;
  const nextId = `${baseId}-next`;
  const expressionText = cadenceToExpression(value);

  function kindLabel(kind: CadenceValue['kind']): string {
    switch (kind) {
      case 'interval':
        return t('wl.schedules.cadence.kindInterval');
      case 'daily':
        return t('wl.schedules.cadence.kindDaily');
      case 'expression':
        return t('wl.schedules.cadence.kindExpression');
    }
  }

  function presetLabel(id: ExpressionPresetId): string {
    switch (id) {
      case 'everyFiveMinutes':
        return t('wl.schedules.cadence.preset.everyFiveMinutes');
      case 'hourly':
        return t('wl.schedules.cadence.preset.hourly');
      case 'dailyNight':
        return t('wl.schedules.cadence.preset.dailyNight');
      case 'weekdayMorning':
        return t('wl.schedules.cadence.preset.weekdayMorning');
    }
  }

  function switchKind(kind: CadenceValue['kind']): void {
    switch (kind) {
      case 'interval':
        onChange({ kind, every: 5, unit: 'minutes' });
        return;
      case 'daily':
        onChange({ kind, time: '02:00' });
        return;
      case 'expression':
        onChange({ kind, expression: expressionText });
        return;
    }
  }

  return (
    <fieldset className={styles.field} disabled={readOnly}>
      <legend className={styles.legend}>{t('wl.schedules.cadence.legend')}</legend>
      <div className={styles.kinds}>
        {KINDS.map((kind) => (
          <label key={kind} className={styles.radio}>
            <input
              type="radio"
              name={kindName}
              value={kind}
              checked={value.kind === kind}
              onChange={() => {
                switchKind(kind);
              }}
            />
            {kindLabel(kind)}
          </label>
        ))}
      </div>
      {value.kind === 'interval' ? (
        <div className={styles.row}>
          <div className={styles.control}>
            <label htmlFor={everyId} className={styles.label}>
              {t('wl.schedules.cadence.every')}
            </label>
            <input
              id={everyId}
              type="number"
              inputMode="numeric"
              min={1}
              step={1}
              className={styles.number}
              aria-invalid={issue === null ? undefined : true}
              aria-describedby={issue === null ? undefined : issueId}
              value={numberText(value.every)}
              onChange={(event: ChangeEvent<HTMLInputElement>) => {
                onChange({ ...value, every: parseNumber(event.target.value) });
              }}
            />
          </div>
          <div className={styles.control}>
            <label htmlFor={unitId} className={styles.label}>
              {t('wl.schedules.cadence.unit')}
            </label>
            <select
              id={unitId}
              className={styles.select}
              value={value.unit}
              onChange={(event) => {
                onChange({ ...value, unit: event.target.value === 'hours' ? 'hours' : 'minutes' });
              }}
            >
              <option value="minutes">{t('wl.schedules.cadence.minutes')}</option>
              <option value="hours">{t('wl.schedules.cadence.hours')}</option>
            </select>
          </div>
          <div role="group" aria-label={t('wl.schedules.cadence.presets')} className={styles.presets}>
            {INTERVAL_PRESETS.map((preset) => {
              const pressed = preset.every === value.every && preset.unit === value.unit;
              const label =
                preset.unit === 'minutes'
                  ? t('wl.schedules.cadence.presetMinutes', { count: preset.every })
                  : t('wl.schedules.cadence.presetHours', { count: preset.every });
              return (
                <button
                  key={`${preset.every}-${preset.unit}`}
                  type="button"
                  className={styles.preset}
                  aria-pressed={pressed}
                  onClick={() => {
                    onChange({ kind: 'interval', every: preset.every, unit: preset.unit });
                  }}
                >
                  {label}
                </button>
              );
            })}
          </div>
        </div>
      ) : null}
      {value.kind === 'daily' ? (
        <div className={styles.row}>
          <div className={styles.control}>
            <label htmlFor={timeId} className={styles.label}>
              {t('wl.schedules.cadence.time')}
            </label>
            <input
              id={timeId}
              type="time"
              className={styles.time}
              aria-invalid={issue === null ? undefined : true}
              aria-describedby={issue === null ? undefined : issueId}
              value={value.time}
              onChange={(event) => {
                onChange({ kind: 'daily', time: event.target.value });
              }}
            />
          </div>
        </div>
      ) : null}
      {value.kind === 'expression' ? (
        <div className={styles.stack}>
          <label htmlFor={expressionId} className={styles.label}>
            {t('wl.schedules.cadence.expression')}
          </label>
          <input
            id={expressionId}
            type="text"
            className={styles.expression}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={issue === null ? undefined : true}
            aria-describedby={issue === null ? undefined : issueId}
            value={value.expression}
            onChange={(event) => {
              onChange({ kind: 'expression', expression: event.target.value });
            }}
          />
          <div role="group" aria-label={t('wl.schedules.cadence.expressionPresets')} className={styles.presets}>
            {EXPRESSION_PRESETS.map((preset) => (
              <button
                key={preset.expression}
                type="button"
                className={styles.preset}
                onClick={() => {
                  onChange({ kind: 'expression', expression: preset.expression });
                }}
              >
                {presetLabel(preset.id)}
              </button>
            ))}
          </div>
        </div>
      ) : null}
      {issue === null ? null : (
        <p id={issueId} className={styles.error}>
          {issueText(issue)}
        </p>
      )}
      <div className={styles.stack}>
        <label htmlFor={timezoneId} className={styles.label}>
          {t('wl.schedules.timezone.label')}
        </label>
        <select
          id={timezoneId}
          className={styles.select}
          aria-invalid={timezoneIssue === null ? undefined : true}
          aria-describedby={timezoneIssue === null ? undefined : timezoneIssueId}
          value={timezone}
          onChange={(event) => {
            onTimezoneChange(event.target.value);
          }}
        >
          {timezones.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        {timezoneIssue === null ? null : (
          <p id={timezoneIssueId} className={styles.error}>
            {issueText(timezoneIssue)}
          </p>
        )}
      </div>
      <section aria-labelledby={nextId} className={styles.next}>
        <h3 id={nextId} className={styles.nextHeading}>
          {t('wl.schedules.next.heading')}
        </h3>
        {nextRuns === null || nextRuns.length === 0 ? (
          <p className={styles.muted}>{t('wl.schedules.next.empty')}</p>
        ) : (
          <ol className={styles.nextList}>
            {nextRuns.map((run) => (
              <li key={run}>
                <time dateTime={new Date(run).toISOString()}>{formatRunTime(run, i18n.language, timezone)}</time>
              </li>
            ))}
          </ol>
        )}
      </section>
    </fieldset>
  );
}
