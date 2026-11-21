import { useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ThemeControl } from '../../../components/ThemeControl/ThemeControl';
import { UndoToast } from '../../../components/UndoToast/UndoToast';
import { LanguageControl } from '../../../i18n/LanguageControl';
import { SAMPLE_ERRORS, SAMPLE_STEPS, STEP_CATALOGUE, STEP_CONTEXT } from './fixtures';
import { createStep, findEntry, insertStep } from './model';
import styles from './StepsSamplePage.module.css';
import { StepList } from './StepList';
import type { RuleStep, StepListHandle, StepRemoval } from './types';

const STEP_LIMIT = 8;

function everyKind(): RuleStep[] {
  return STEP_CATALOGUE.map((entry) => createStep(entry, `kind-${entry.kind}`));
}

export function StepsSamplePage() {
  const { t } = useTranslation();
  const listRef = useRef<StepListHandle>(null);
  const [steps, setSteps] = useState<RuleStep[]>(() => SAMPLE_STEPS.map((step) => structuredClone(step)));
  const [kinds, setKinds] = useState<RuleStep[]>(everyKind);
  const [showErrors, setShowErrors] = useState(false);
  const [readOnly, setReadOnly] = useState(false);
  const [highlight, setHighlight] = useState(false);
  const [removal, setRemoval] = useState<StepRemoval | null>(null);

  const order = useMemo(
    () => steps.map((step) => findEntry(STEP_CATALOGUE, step.kind)?.title ?? step.kind).join(' > '),
    [steps],
  );

  return (
    <main className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('wl.steps.sample.title')}</h1>
        <LanguageControl />
        <ThemeControl />
      </header>

      <section className={styles.section} aria-labelledby="steps-sample-draft">
        <h2 id="steps-sample-draft" className={styles.heading}>
          {t('wl.steps.sample.draft')}
        </h2>
        <div className={styles.toolbar}>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={showErrors}
              onChange={(event) => {
                setShowErrors(event.target.checked);
              }}
            />
            <span>{t('wl.steps.sample.showErrors')}</span>
          </label>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={readOnly}
              onChange={(event) => {
                setReadOnly(event.target.checked);
              }}
            />
            <span>{t('wl.steps.sample.readOnly')}</span>
          </label>
          <label className={styles.toggle}>
            <input
              type="checkbox"
              checked={highlight}
              onChange={(event) => {
                setHighlight(event.target.checked);
              }}
            />
            <span>{t('wl.steps.sample.highlight')}</span>
          </label>
          <button
            type="button"
            className={styles.button}
            onClick={() => {
              listRef.current?.focusStep('sample-http', 'url');
            }}
          >
            {t('wl.steps.sample.focusError')}
          </button>
        </div>
        <StepList
          ref={listRef}
          steps={steps}
          catalogue={STEP_CATALOGUE}
          context={STEP_CONTEXT}
          onChange={setSteps}
          errors={showErrors ? SAMPLE_ERRORS : undefined}
          maxSteps={STEP_LIMIT}
          readOnly={readOnly}
          highlightedStepId={highlight ? 'sample-respond' : null}
          onStepRemoved={setRemoval}
        />
        <p className={styles.note} data-testid="steps-order">
          {t('wl.steps.sample.order', { order })}
        </p>
        {removal === null ? null : (
          <UndoToast
            message={t('wl.steps.sample.removed', {
              title: findEntry(STEP_CATALOGUE, removal.step.kind)?.title ?? removal.step.kind,
            })}
            onUndo={() => {
              setSteps((current) => insertStep(current, removal.index, removal.step));
              setRemoval(null);
            }}
            onDismiss={() => {
              setRemoval(null);
            }}
          />
        )}
      </section>

      <section className={styles.section} aria-labelledby="steps-sample-kinds">
        <h2 id="steps-sample-kinds" className={styles.heading}>
          {t('wl.steps.sample.kinds')}
        </h2>
        <StepList
          steps={kinds}
          catalogue={STEP_CATALOGUE}
          context={STEP_CONTEXT}
          onChange={setKinds}
          label={t('wl.steps.sample.kinds')}
        />
      </section>
    </main>
  );
}
