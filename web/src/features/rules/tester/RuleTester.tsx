import { useId, useMemo, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { CapturePicker } from '../../../components/CapturePicker/CapturePicker';
import { useNow } from '../../endpoints/useNow';
import { formatElapsedTime } from '../../../components/format/format';
import { FinalResponsePanel } from './FinalResponsePanel';
import { RealRunPanel } from './RealRunPanel';
import { describeFailure } from './summary';
import { TestAllSummary } from './TestAllSummary';
import styles from './Tester.module.css';
import { selectionOf } from './trace';
import { TracePanel } from './TracePanel';
import { useRuleTester } from './useRuleTester';
import { VariablesInspector } from './VariablesInspector';
import { variableRowsAt } from './variables';
import type { RuleTesterProps } from './props';
import type { ErrorPosition, TesterScope, TraceGroup, TraceItem } from './types';

function isEditableTarget(target: EventTarget): boolean {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    target.isContentEditable ||
    target.closest('.cm-editor') !== null
  );
}

export function RuleTester(props: RuleTesterProps) {
  const { t, i18n } = useTranslation();
  const {
    endpointId,
    endpointUrl,
    showScope = true,
    blocked = false,
    dirty = false,
    canEdit = true,
    stepTitles,
    onGoToError,
    onInsertVariable,
    draft,
  } = props;
  const model = useRuleTester(props);
  const now = useNow();
  const headingId = useId();
  const scopeName = useId();
  const { trace, cursor, runner } = model;
  const rows = useMemo(
    () => (trace === null ? [] : variableRowsAt(trace, model.capture, cursor.selected)),
    [trace, model.capture, cursor.selected],
  );
  const changes = useMemo(() => rows.filter((row) => row.change !== 'same' && row.group !== 'request'), [rows]);
  const last = trace === null ? 0 : trace.items.length - 1;
  const failure = runner.phase === 'failed' ? describeFailure(runner.error, t) : null;
  const target = model.capture === undefined ? endpointUrl : `${endpointUrl}${model.capture.path}`;

  function handleKeyDown(event: KeyboardEvent<HTMLElement>): void {
    if (event.altKey || event.ctrlKey || event.metaKey || isEditableTarget(event.target)) {
      return;
    }
    if (event.key === ']') {
      event.preventDefault();
      model.next();
    } else if (event.key === '[') {
      event.preventDefault();
      model.back();
    }
  }

  function goTo(item: TraceItem, position: ErrorPosition): void {
    onGoToError?.(selectionOf(item), position);
  }

  function openGroup(group: TraceGroup): void {
    const first = trace?.items.findIndex((item) => item.group === group.index) ?? -1;
    if (first >= 0) {
      model.select(first);
    }
  }

  let verdict = '';
  if (trace !== null) {
    verdict = trace.failed
      ? t('wl.tester.status.failed')
      : trace.matchedCount > 0
        ? t('wl.tester.status.matched')
        : t('wl.tester.status.notMatched');
  }

  let statusText = '';
  if (model.running) {
    statusText = t('wl.tester.status.running');
  } else if (model.waiting) {
    statusText = t('wl.tester.status.waiting');
  } else if (trace !== null) {
    statusText = t('wl.tester.status.summary', {
      when: formatElapsedTime(new Date(runner.at).toISOString(), now, i18n.language),
      verdict,
      ms: trace.durationMs,
    });
  }

  let hint = '';
  if (!canEdit) {
    hint = t('wl.tester.hint.viewer');
  } else if (blocked) {
    hint = t('wl.tester.hint.blocked');
  } else if (model.scope === 'rule' && draft === null) {
    hint = t('wl.tester.hint.noDraft');
  } else if (model.noRules) {
    hint = t('wl.tester.hint.noRules');
  } else if (model.captureId === null && model.captures !== undefined) {
    hint = t('wl.tester.hint.noCapture');
  }

  const canStep = trace !== null && cursor.selected < last;

  return (
    <section className={styles.tester} aria-labelledby={headingId} onKeyDown={handleKeyDown}>
      <div className={styles.header}>
        <h2 id={headingId} className={styles.title}>
          {t('wl.tester.title')}
        </h2>
        {showScope ? (
          <fieldset className={styles.scope}>
            <legend className={styles.live}>{t('wl.tester.scope.label')}</legend>
            {(['rule', 'all'] as const).map((value: TesterScope) => (
              <label key={value} className={styles.scopeChoice}>
                <input
                  type="radio"
                  name={scopeName}
                  value={value}
                  checked={model.scope === value}
                  onChange={() => {
                    model.setScope(value);
                  }}
                />
                {value === 'rule' ? t('wl.tester.scope.rule') : t('wl.tester.scope.all')}
              </label>
            ))}
          </fieldset>
        ) : null}
      </div>

      <CapturePicker
        captures={model.captures}
        value={model.pick}
        onChange={model.setPick}
        onSendSample={model.sendSample}
        sendingSample={model.sendingSample}
        now={now}
      />

      <div className={styles.controls}>
        <button
          type="button"
          className={`${styles.button} ${styles.primary}`}
          disabled={!model.canRun || model.running}
          aria-keyshortcuts="Control+Enter Meta+Enter"
          onClick={model.run}
        >
          {t('wl.tester.run')}
        </button>
        <button type="button" className={styles.button} disabled={!canStep} aria-keyshortcuts="]" onClick={model.next}>
          {t('wl.tester.step')}
        </button>
        <button
          type="button"
          className={styles.button}
          disabled={trace === null || cursor.selected === 0}
          aria-keyshortcuts="["
          onClick={model.back}
        >
          {t('wl.tester.back')}
        </button>
        <button type="button" className={styles.button} disabled={trace === null} onClick={model.reset}>
          {t('wl.tester.reset')}
        </button>
        <label className={styles.toggle}>
          <input
            type="checkbox"
            checked={model.auto}
            onChange={(event) => {
              model.setAuto(event.target.checked);
            }}
          />
          {t('wl.tester.auto')}
        </label>
      </div>

      <p className={styles.statusLine}>
        {statusText}
        {model.stale && trace !== null ? <span className={styles.stale}>{t('wl.tester.stale')}</span> : null}
      </p>
      <div role="status" aria-live="polite" className={styles.live}>
        <span key={runner.resultId}>
          {model.stepAnnouncement === '' ? model.runAnnouncement : model.stepAnnouncement}
        </span>
      </div>
      {hint === '' ? null : <p className={styles.note}>{hint}</p>}
      {model.sampleError === null || model.sampleError === undefined ? null : (
        <p role="alert" className={styles.error}>
          {t('wl.tester.sampleFailed', { message: describeFailure(model.sampleError, t).message })}
        </p>
      )}
      {failure === null ? null : (
        <div role="alert" className={styles.error}>
          <span>{t('wl.tester.error.run', { message: failure.message })}</span>
          {failure.fields.length === 0 ? null : (
            <ul className={styles.errorList}>
              {failure.fields.map((field) => (
                <li key={field}>{field}</li>
              ))}
            </ul>
          )}
        </div>
      )}
      {model.savedRulesFailed ? (
        <p role="alert" className={styles.error}>
          {t('wl.tester.error.rules')}
        </p>
      ) : null}

      {trace === null ? null : (
        <>
          {model.scope === 'all' ? <TestAllSummary trace={trace} onOpen={openGroup} /> : null}
          <TracePanel
            trace={trace}
            cursor={cursor}
            showRule={model.scope === 'all'}
            stepTitles={stepTitles}
            changes={changes}
            onSelect={model.select}
            onGoTo={onGoToError === undefined ? undefined : goTo}
          />
          <VariablesInspector rows={rows} position={cursor.selected + 1} onInsert={onInsertVariable} />
          <FinalResponsePanel final={trace.final} ruleCount={trace.groups.length} error={trace.error} />
        </>
      )}

      <RealRunPanel
        endpointId={endpointId}
        target={target}
        effects={model.effects}
        stepTitles={stepTitles}
        dirty={dirty}
        disabled={!canEdit || model.capture === undefined}
        open={model.realOpen}
        pending={model.realPending}
        result={model.realResult}
        error={model.realError}
        onOpenChange={model.setRealOpen}
        onConfirm={model.confirmReal}
      />
    </section>
  );
}
