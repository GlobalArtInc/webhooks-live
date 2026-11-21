import { useEffect, useImperativeHandle, useRef, useState, type Ref } from 'react';
import { useTranslation } from 'react-i18next';
import { AddStepMenu } from './AddStepMenu';
import { createStep, findEntry, insertStep, moveStep, removeStep, replaceStep } from './model';
import { StepCard } from './StepCard';
import styles from './StepList.module.css';
import type { RuleStep, StepCatalogueEntry, StepEditorContext, StepError, StepListHandle, StepRemoval } from './types';

export interface StepListProps {
  steps: readonly RuleStep[];
  catalogue: readonly StepCatalogueEntry[];
  context: StepEditorContext;
  onChange: (steps: RuleStep[]) => void;
  errors?: readonly StepError[];
  maxSteps?: number;
  readOnly?: boolean;
  highlightedStepId?: string | null;
  onStepRemoved?: (removal: StepRemoval) => void;
  label?: string;
  ref?: Ref<StepListHandle>;
}

interface PendingFocus {
  kind: 'heading' | 'field' | 'add';
  stepId: string | null;
  field: string | null;
}

const NO_ERRORS: readonly StepError[] = [];
const FOCUS_ATTEMPTS = 40;
const FOCUSABLE = 'input:not(:disabled), select:not(:disabled), textarea:not(:disabled), .cm-content';

function tryFocus(list: HTMLElement | null, cards: Map<string, HTMLElement>, pending: PendingFocus): boolean {
  if (pending.kind === 'add') {
    const button = list?.querySelector<HTMLElement>('[data-add-step]:not(:disabled)') ?? null;
    button?.focus();
    return button !== null;
  }
  const card = pending.stepId === null ? undefined : cards.get(pending.stepId);
  if (card === undefined) {
    return false;
  }
  if (pending.kind === 'heading') {
    const heading = card.querySelector<HTMLElement>('[data-step-heading]');
    heading?.focus();
    return heading !== null;
  }
  const selector =
    pending.field === null ? '[data-schema-field][data-kind]' : `[data-schema-field="${CSS.escape(pending.field)}"]`;
  const root = card.querySelector<HTMLElement>(selector);
  if (root === null) {
    if (pending.field === null) {
      card.querySelector<HTMLElement>('[data-step-heading]')?.focus();
      return true;
    }
    return false;
  }
  const target = root.querySelector<HTMLElement>(FOCUSABLE);
  target?.focus();
  return target !== null && document.activeElement === target;
}

export function StepList({
  steps,
  catalogue,
  context,
  onChange,
  errors = NO_ERRORS,
  maxSteps,
  readOnly = false,
  highlightedStepId = null,
  onStepRemoved,
  label,
  ref,
}: StepListProps) {
  const { t } = useTranslation();
  const listRef = useRef<HTMLDivElement>(null);
  const cards = useRef(new Map<string, HTMLElement>());
  const pendingFocus = useRef<PendingFocus | null>(null);
  const pendingRefocus = useRef<HTMLElement | null>(null);
  const frame = useRef(0);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [announcement, setAnnouncement] = useState('');

  const atLimit = maxSteps !== undefined && steps.length >= maxSteps;

  function titleOf(step: RuleStep): string {
    return findEntry(catalogue, step.kind)?.title ?? step.kind;
  }

  function expand(stepId: string): void {
    setExpanded((current) => new Set(current).add(stepId));
  }

  function collapse(stepId: string): void {
    setExpanded((current) => {
      const next = new Set(current);
      next.delete(stepId);
      return next;
    });
  }

  useImperativeHandle(ref, () => ({
    focusStep: (stepId, field) => {
      expand(stepId);
      pendingFocus.current = { kind: 'field', stepId, field: field ?? null };
    },
  }));

  useEffect(() => {
    const refocus = pendingRefocus.current;
    if (refocus !== null) {
      pendingRefocus.current = null;
      if (document.activeElement !== refocus && refocus.isConnected) {
        refocus.focus();
      }
    }
    const pending = pendingFocus.current;
    if (pending === null) {
      return;
    }
    pendingFocus.current = null;
    let attempts = 0;
    const attempt = (): void => {
      attempts += 1;
      if (!tryFocus(listRef.current, cards.current, pending) && attempts < FOCUS_ATTEMPTS) {
        frame.current = requestAnimationFrame(attempt);
      }
    };
    attempt();
  });

  useEffect(
    () => () => {
      cancelAnimationFrame(frame.current);
    },
    [],
  );

  function addAt(index: number, kind: string): void {
    const entry = findEntry(catalogue, kind);
    if (entry === undefined || atLimit) {
      return;
    }
    const step = createStep(entry);
    expand(step.id);
    pendingFocus.current = { kind: 'field', stepId: step.id, field: null };
    setAnnouncement(t('wl.steps.announce.added', { title: entry.title, position: index + 1, total: steps.length + 1 }));
    onChange(insertStep(steps, index, step));
  }

  function moveAt(index: number, delta: number): void {
    const target = index + delta;
    const step = steps[index];
    if (step === undefined || target < 0 || target >= steps.length) {
      return;
    }
    const active = document.activeElement;
    pendingRefocus.current = active instanceof HTMLElement ? active : null;
    setAnnouncement(t('wl.steps.announce.moved', { title: titleOf(step), position: target + 1, total: steps.length }));
    onChange(moveStep(steps, index, delta));
  }

  function removeAt(index: number): void {
    const step = steps[index];
    if (step === undefined) {
      return;
    }
    const neighbour = steps[index + 1] ?? steps[index - 1];
    pendingFocus.current =
      neighbour === undefined
        ? { kind: 'add', stepId: null, field: null }
        : { kind: 'heading', stepId: neighbour.id, field: null };
    collapse(step.id);
    setAnnouncement(t('wl.steps.announce.removed', { title: titleOf(step), position: index + 1 }));
    onChange(removeStep(steps, index));
    onStepRemoved?.({ step, index });
  }

  function toggle(stepId: string): void {
    if (expanded.has(stepId)) {
      collapse(stepId);
    } else {
      expand(stepId);
    }
  }

  function collapseAndFocus(stepId: string): void {
    pendingFocus.current = { kind: 'heading', stepId, field: null };
    collapse(stepId);
  }

  const limitText = maxSteps === undefined ? null : t('wl.steps.list.count', { used: steps.length, max: maxSteps });

  return (
    <div className={styles.root} ref={listRef}>
      {steps.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyText}>{t('wl.steps.list.empty')}</p>
          {readOnly ? null : (
            <AddStepMenu
              catalogue={catalogue}
              position={1}
              disabled={atLimit}
              onAdd={(kind) => {
                addAt(0, kind);
              }}
            />
          )}
        </div>
      ) : (
        <ol className={styles.list} aria-label={label ?? t('wl.steps.list.label')}>
          {steps.map((step, index) => (
            <li key={step.id} className={styles.item}>
              <StepCard
                step={step}
                entry={findEntry(catalogue, step.kind)}
                index={index}
                total={steps.length}
                expanded={expanded.has(step.id)}
                highlighted={step.id === highlightedStepId}
                readOnly={readOnly}
                context={context}
                errors={errors.filter((error) => error.stepId === step.id)}
                takenNames={steps
                  .filter((other) => other.id !== step.id)
                  .flatMap((other) => (other.name === null ? [] : [other.name]))}
                onToggle={() => {
                  toggle(step.id);
                }}
                onCollapse={() => {
                  collapseAndFocus(step.id);
                }}
                onChange={(next) => {
                  onChange(replaceStep(steps, index, next));
                }}
                onMove={(delta) => {
                  moveAt(index, delta);
                }}
                onRemove={() => {
                  removeAt(index);
                }}
                registerElement={(element) => {
                  if (element === null) {
                    cards.current.delete(step.id);
                  } else {
                    cards.current.set(step.id, element);
                  }
                }}
              />
              {readOnly ? null : (
                <div className={styles.slot}>
                  <AddStepMenu
                    catalogue={catalogue}
                    position={index + 2}
                    disabled={atLimit}
                    disabledReason={atLimit ? t('wl.steps.list.limitReached') : undefined}
                    onAdd={(kind) => {
                      addAt(index + 1, kind);
                    }}
                  />
                </div>
              )}
            </li>
          ))}
        </ol>
      )}
      {limitText === null ? null : (
        <p className={styles.count} data-full={atLimit}>
          {atLimit ? `${t('wl.steps.list.limitReached')} ` : ''}
          {limitText}
        </p>
      )}
      {steps.length > 1 && !readOnly ? <p className={styles.hint}>{t('wl.steps.list.moveHint')}</p> : null}
      <div role="status" className={styles.visuallyHidden}>
        {announcement}
      </div>
    </div>
  );
}
