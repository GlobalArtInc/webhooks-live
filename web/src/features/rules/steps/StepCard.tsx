import { useId, useMemo, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { describeGuard } from './guardText';
import { issueMessage } from './issueMessage';
import { producedVariables, renderSummary, setStepField, validateStep } from './model';
import { SchemaField } from './SchemaField';
import styles from './StepCard.module.css';
import { StepGuardEditor } from './StepGuardEditor';
import type { FieldValue, RuleStep, StepCatalogueEntry, StepEditorContext, StepError } from './types';

export interface StepCardProps {
  step: RuleStep;
  entry: StepCatalogueEntry | undefined;
  index: number;
  total: number;
  expanded: boolean;
  onToggle: () => void;
  onCollapse: () => void;
  onChange: (step: RuleStep) => void;
  onMove: (delta: number) => void;
  onRemove: () => void;
  errors: readonly StepError[];
  takenNames: readonly string[];
  context: StepEditorContext;
  highlighted: boolean;
  readOnly: boolean;
  registerElement: (element: HTMLElement | null) => void;
}

interface CardMessage {
  field: string | null;
  message: string;
}

function isHeadingButton(target: EventTarget): boolean {
  return target instanceof HTMLElement && target.hasAttribute('data-step-heading');
}

function isEscapeSafe(target: EventTarget): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  return target.closest('.cm-editor') === null && target.tagName !== 'SELECT';
}

export function StepCard({
  step,
  entry,
  index,
  total,
  expanded,
  onToggle,
  onCollapse,
  onChange,
  onMove,
  onRemove,
  errors,
  takenNames,
  context,
  highlighted,
  readOnly,
  registerElement,
}: StepCardProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const bodyId = `${baseId}-body`;
  const statusId = `${baseId}-status`;
  const title = entry?.title ?? step.kind;
  const position = index + 1;

  const messages = useMemo<CardMessage[]>(() => {
    const own = errors.map((error): CardMessage => ({ field: error.field, message: error.message }));
    const local = validateStep(entry, step, takenNames).map((item): CardMessage => ({
      field: item.field,
      message: issueMessage(t, item.issue),
    }));
    return [...own, ...local];
  }, [errors, entry, step, takenNames, t]);

  const invalid = messages.length > 0;
  const homes = new Set<string>(['guard', 'name', ...(entry?.fields ?? []).map((field) => field.name)]);
  const stepLevel = messages.filter((item) => item.field === null || !homes.has(item.field));

  function messageFor(field: string): string | null {
    const found = messages.filter((item) => item.field === field).map((item) => item.message);
    return found.length === 0 ? null : found.join(' ');
  }

  const summary = entry === undefined ? null : renderSummary(entry, step);
  const produced = entry === undefined ? [] : producedVariables(entry, step);

  function handleKeyDown(event: KeyboardEvent<HTMLElement>): void {
    const target = event.target;
    if (event.defaultPrevented || !(target instanceof Node) || !event.currentTarget.contains(target)) {
      return;
    }
    if (event.altKey && event.shiftKey && (event.key === 'ArrowUp' || event.key === 'ArrowDown')) {
      event.preventDefault();
      if (!readOnly) {
        onMove(event.key === 'ArrowUp' ? -1 : 1);
      }
      return;
    }
    if (event.key === 'Escape' && expanded && isEscapeSafe(target)) {
      event.preventDefault();
      onCollapse();
      return;
    }
    if (event.key === 'Delete' && !readOnly && isHeadingButton(target)) {
      event.preventDefault();
      onRemove();
    }
  }

  function phaseBadge() {
    if (entry === undefined || entry.phase === 'any') {
      return null;
    }
    return (
      <span className={styles.badge} data-phase={entry.phase}>
        {entry.phase === 'before' ? t('wl.steps.phase.before') : t('wl.steps.phase.after')}
      </span>
    );
  }

  return (
    <section
      ref={registerElement}
      className={styles.card}
      aria-label={t('wl.steps.card.label', { position, total, title })}
      aria-describedby={invalid ? statusId : undefined}
      data-step-id={step.id}
      data-expanded={expanded}
      data-invalid={invalid}
      data-highlighted={highlighted}
      onKeyDown={handleKeyDown}
    >
      <div className={styles.header}>
        <h3 className={styles.heading}>
          <button
            type="button"
            className={styles.toggle}
            data-step-heading=""
            aria-expanded={expanded}
            aria-controls={bodyId}
            onClick={onToggle}
          >
            <span className={styles.chevron} aria-hidden="true">
              {expanded ? '▾' : '▸'}
            </span>
            <span className={styles.number}>{position}</span>
            <span className={styles.title}>{title}</span>
          </button>
        </h3>
        {readOnly ? null : (
          <div className={styles.actions}>
            <button
              type="button"
              className={styles.action}
              aria-label={t('wl.steps.card.moveUp', { position })}
              aria-disabled={index === 0}
              onClick={() => {
                onMove(-1);
              }}
            >
              <span aria-hidden="true">{'↑'}</span>
            </button>
            <button
              type="button"
              className={styles.action}
              aria-label={t('wl.steps.card.moveDown', { position })}
              aria-disabled={index === total - 1}
              onClick={() => {
                onMove(1);
              }}
            >
              <span aria-hidden="true">{'↓'}</span>
            </button>
            <button
              type="button"
              className={styles.action}
              aria-label={t('wl.steps.card.remove', { position, title })}
              onClick={onRemove}
            >
              <span aria-hidden="true">{'×'}</span>
            </button>
          </div>
        )}
      </div>
      <div className={styles.meta}>
        {phaseBadge()}
        {invalid ? (
          <span id={statusId} className={styles.errorCount}>
            {t('wl.steps.card.errors', { count: messages.length })}
          </span>
        ) : null}
        {entry === undefined ? (
          <span className={styles.unknown}>{t('wl.steps.card.unknownKind', { kind: step.kind })}</span>
        ) : null}
      </div>
      {expanded ? null : summary === null ? null : <p className={styles.summary}>{summary}</p>}
      {expanded || step.guard === null ? null : <p className={styles.guardLine}>{describeGuard(t, step.guard)}</p>}
      {stepLevel.length > 0 ? (
        <ul className={styles.stepErrors}>
          {stepLevel.map((item, itemIndex) => (
            <li key={itemIndex}>{item.message}</li>
          ))}
        </ul>
      ) : null}
      <div id={bodyId} className={styles.body} hidden={!expanded}>
        {expanded ? (
          <>
            {entry?.description === undefined || entry.description === '' ? null : (
              <p className={styles.description}>{entry.description}</p>
            )}
            {(entry?.fields ?? []).map((field) => (
              <SchemaField
                key={field.name}
                field={field}
                value={step.fields[field.name] ?? null}
                idPrefix={baseId}
                context={context}
                readOnly={readOnly}
                error={messageFor(field.name)}
                onChange={(value: FieldValue) => {
                  onChange(setStepField(step, field.name, value));
                }}
              />
            ))}
            {context.guards === false ? null : (
              <StepGuardEditor
                guard={step.guard}
                idPrefix={baseId}
                readOnly={readOnly}
                error={messageFor('guard')}
                onChange={(guard) => {
                  onChange({ ...step, guard });
                }}
              />
            )}
            {produced.length === 0 ? null : (
              <p className={styles.produces}>
                <span>{t('wl.steps.produces.label')}</span>{' '}
                {produced.map((name) => (
                  <code key={name} className={styles.code}>
                    {name}
                  </code>
                ))}
              </p>
            )}
            <div className={styles.advanced} data-schema-field="name">
              <label className={styles.nameLabel}>
                <span>{t('wl.steps.advanced.name')}</span>
                <input
                  type="text"
                  className={styles.nameInput}
                  value={step.name ?? ''}
                  readOnly={readOnly}
                  autoComplete="off"
                  autoCapitalize="off"
                  spellCheck={false}
                  aria-invalid={messageFor('name') !== null}
                  aria-describedby={`${baseId}-name-help`}
                  onChange={(event) => {
                    onChange({ ...step, name: event.target.value === '' ? null : event.target.value });
                  }}
                />
              </label>
              <p id={`${baseId}-name-help`} className={styles.hint} data-error={messageFor('name') !== null}>
                {messageFor('name') ?? t('wl.steps.advanced.nameHelp')}
              </p>
              <label className={styles.check}>
                <input
                  type="checkbox"
                  checked={step.continueOnError}
                  disabled={readOnly}
                  onChange={(event) => {
                    onChange({ ...step, continueOnError: event.target.checked });
                  }}
                />
                <span>{t('wl.steps.advanced.continueOnError')}</span>
              </label>
            </div>
          </>
        ) : null}
      </div>
    </section>
  );
}
