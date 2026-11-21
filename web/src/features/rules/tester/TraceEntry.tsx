import type { TFunction } from 'i18next';
import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { BoundedText } from './BoundedText';
import { statusGlyph, statusLabel, stepTitle } from './labels';
import { clip } from './parse';
import { outputSummary, scriptLog } from './summary';
import styles from './Tester.module.css';
import { errorPosition } from './trace';
import type { ConditionResult, ErrorPosition, TraceItem, VariableRow } from './types';

export interface TraceEntryProps {
  item: TraceItem;
  index: number;
  selected: boolean;
  revealed: boolean;
  showRule: boolean;
  stepTitles: Readonly<Record<string, string>> | undefined;
  changes: readonly VariableRow[];
  onSelect: (index: number) => void;
  onGoTo: ((item: TraceItem, position: ErrorPosition) => void) | undefined;
}

const REASON_CHARS = 80;

function conditionLine(condition: ConditionResult, t: TFunction): string {
  return t('wl.tester.conditions.line', {
    subject: condition.subject,
    op: condition.op,
    expected: condition.expected ?? '',
    actual: condition.actual ?? t('wl.tester.conditions.missing'),
    result: condition.result ? t('wl.tester.conditions.true') : t('wl.tester.conditions.false'),
  });
}

export function TraceEntry({
  item,
  index,
  selected,
  revealed,
  showRule,
  stepTitles,
  changes,
  onSelect,
  onGoTo,
}: TraceEntryProps) {
  const { t, i18n } = useTranslation();
  const [viewing, setViewing] = useState(false);
  const bodyId = useId();
  const status = revealed ? item.status : 'notReached';
  const reached = status !== 'notReached';
  const statusText =
    status === 'failed' && item.detail !== null
      ? t('wl.tester.status.failedWith', { reason: clip(item.detail, REASON_CHARS) })
      : status === 'skipped' && item.detail !== null
        ? t('wl.tester.status.skippedWith', { reason: clip(item.detail, REASON_CHARS) })
        : statusLabel(status, t);
  const title =
    item.type === 'rule'
      ? showRule
        ? t('wl.tester.conditions.ofRule', { name: item.ruleName })
        : t('wl.tester.conditions.title')
      : `${item.position ?? ''} ${stepTitle(item.stepKind, stepTitles)}${item.stepName === null ? '' : ` (${item.stepName})`}`;
  const position = item.status === 'failed' ? errorPosition(item.detail) : null;
  const summary = item.type === 'step' && item.status !== 'notReached' ? outputSummary(item, t, i18n.language) : '';
  const log = item.type === 'step' ? scriptLog(item) : [];
  const conditions = item.conditions;
  const passed = conditions === null ? 0 : conditions.filter((condition) => condition.result).length;
  const outputText = item.output === null ? '' : JSON.stringify(item.output, null, 2);

  return (
    <li
      className={styles.entry}
      data-status={status}
      data-testid={`trace-${item.id}`}
      aria-current={selected ? 'step' : undefined}
    >
      <button
        type="button"
        className={styles.entryHead}
        aria-expanded={selected && reached}
        aria-controls={selected && reached ? bodyId : undefined}
        disabled={!revealed}
        onClick={() => {
          onSelect(index);
        }}
      >
        <span className={styles.glyph} aria-hidden="true">
          {statusGlyph(status)}
        </span>
        <span className={styles.entryTitle}>{title}</span>
        <span className={styles.entryStatus}>{statusText}</span>
        {item.type === 'step' && reached && !item.placeholder ? (
          <span className={styles.duration}>{t('wl.tester.duration', { ms: item.durationMs })}</span>
        ) : null}
      </button>
      {selected && reached ? (
        <div id={bodyId} className={styles.entryBody}>
          {item.type === 'rule' ? (
            <>
              {conditions === null ? (
                <p className={styles.note}>{t('wl.tester.conditions.noDetail')}</p>
              ) : conditions.length === 0 ? (
                <p className={styles.note}>{t('wl.tester.conditions.always')}</p>
              ) : (
                <>
                  <p className={styles.note}>{t('wl.tester.conditions.count', { passed, total: conditions.length })}</p>
                  <ul className={styles.rows}>
                    {conditions.map((condition, position) => (
                      <li key={`${condition.subject}:${position}`} className={styles.path}>
                        {conditionLine(condition, t)}
                      </li>
                    ))}
                  </ul>
                </>
              )}
            </>
          ) : (
            <>
              {summary === '' ? null : <p className={styles.summaryText}>{summary}</p>}
              {changes.length === 0 ? null : (
                <ul className={styles.rows} aria-label={t('wl.tester.changes.label')}>
                  {changes.map((row) => (
                    <li key={row.path} className={styles.row}>
                      <span className={styles.marker} data-change={row.change} aria-hidden="true">
                        {row.change === 'new' ? '+' : '~'}
                      </span>
                      <span className={styles.path}>{row.path}</span>
                      {row.known ? <span className={styles.value}>{clip(row.text, 200)}</span> : null}
                      <span className={styles.entryStatus}>
                        {row.change === 'new' ? t('wl.tester.variables.new') : t('wl.tester.variables.changed')}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
              {log.length === 0 ? null : (
                <>
                  <p className={styles.sectionTitle}>{t('wl.tester.log.title')}</p>
                  <BoundedText text={log.join('\n')} label={t('wl.tester.log.label')} />
                </>
              )}
              {position === null || onGoTo === undefined ? null : (
                <button
                  type="button"
                  className={`${styles.button} ${styles.small}`}
                  onClick={() => {
                    onGoTo(item, position);
                  }}
                >
                  {t('wl.tester.error.go', { line: position.line, column: position.column })}
                </button>
              )}
              {item.output === null ? null : (
                <>
                  <button
                    type="button"
                    className={`${styles.button} ${styles.small}`}
                    aria-expanded={viewing}
                    onClick={() => {
                      setViewing((value) => !value);
                    }}
                  >
                    {viewing ? t('wl.tester.output.hide') : t('wl.tester.output.view')}
                  </button>
                  {viewing ? (
                    <BoundedText
                      text={outputText}
                      label={
                        item.status === 'simulated' ? t('wl.tester.output.simulated') : t('wl.tester.output.label')
                      }
                    />
                  ) : null}
                </>
              )}
              {item.type === 'step' && item.status === 'deferred' ? (
                <p className={styles.note}>{t('wl.tester.deferred.note')}</p>
              ) : null}
            </>
          )}
        </div>
      ) : null}
    </li>
  );
}
