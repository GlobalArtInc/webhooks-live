import { useId, useMemo, type FocusEvent, type Ref } from 'react';
import { useTranslation } from 'react-i18next';
import { ConditionGroup } from '../../../components/ConditionGroup/ConditionGroup';
import type { VariableEntry } from '../../../components/VariablePicker/variables';
import type { CodeEditorHandle } from '../../editor/types';
import { DEFAULT_LIMITS, TRIGGER_KINDS } from '../conditions/constants';
import { ScriptStepEditor } from '../script/ScriptStepEditor';
import { StepList } from '../steps/StepList';
import { setStepField } from '../steps/model';
import type { StepEditorContext, StepError, StepListHandle, StepRemoval } from '../steps/types';
import { RULE_CATALOGUE } from './catalogue';
import { RULE_CONDITION_CATALOGUE } from './conditionCatalogue';
import { mergeIssues, phaseSplit } from './model';
import styles from './RuleEditor.module.css';
import { RULE_STEP_CATALOGUE } from './stepCatalogue';
import type { RuleDraftState } from './useRuleDraft';

export interface BuildPanelProps {
  state: RuleDraftState;
  readOnly: boolean;
  scriptRule: boolean;
  context: StepEditorContext;
  variables: readonly VariableEntry[];
  highlightedStepId: string | null;
  conditionsHighlighted: boolean;
  conditionsRef: Ref<HTMLElement>;
  listRef: Ref<StepListHandle>;
  scriptRef: Ref<CodeEditorHandle>;
  onStepRemoved: (removal: StepRemoval) => void;
  onStepsFocus: (event: FocusEvent<HTMLElement>) => void;
}

export function BuildPanel({
  state,
  readOnly,
  scriptRule,
  context,
  variables,
  highlightedStepId,
  conditionsHighlighted,
  conditionsRef,
  listRef,
  scriptRef,
  onStepRemoved,
  onStepsFocus,
}: BuildPanelProps) {
  const { t } = useTranslation();
  const conditionsId = useId();
  const stepsId = useId();
  const { draft, validation, server, attempted } = state;
  const issues = useMemo(
    () => mergeIssues(validation.conditions.rows, server.rows),
    [validation.conditions.rows, server.rows],
  );
  const stepErrors = useMemo<StepError[]>(() => {
    const fromServer = server.steps.map((error) => ({
      stepId: error.stepId,
      field: error.field,
      message: error.message,
    }));
    if (!attempted) {
      return fromServer;
    }
    const local = validation.steps.flatMap((problem): StepError[] => {
      if (problem.kind === 'required') {
        return [{ stepId: problem.stepId, field: problem.field, message: t('wl.ruleeditor.issue.required') }];
      }
      if (problem.kind === 'unsupportedName') {
        return [{ stepId: problem.stepId, field: null, message: t('wl.ruleeditor.issue.stepNameUnsupported') }];
      }
      return [];
    });
    return [...fromServer, ...local];
  }, [server.steps, validation.steps, attempted, t]);
  const split = phaseSplit(draft.steps, RULE_CATALOGUE);
  const [scriptStep] = draft.steps;
  const scriptText = scriptStep === undefined ? '' : String(scriptStep.fields.source ?? '');

  return (
    <>
      <section
        ref={conditionsRef}
        className={styles.section}
        aria-labelledby={conditionsId}
        data-highlighted={conditionsHighlighted}
      >
        <h3 id={conditionsId} className={styles.heading}>
          {t('wl.ruleeditor.conditions.title')}
        </h3>
        <p className={styles.hint}>{t('wl.ruleeditor.conditions.hint')}</p>
        <ConditionGroup
          root={draft.root}
          onChange={state.setRoot}
          catalogue={RULE_CONDITION_CATALOGUE}
          triggers={TRIGGER_KINDS}
          variables={variables}
          issues={issues}
          showIssues={attempted || server.rows.size > 0}
          readOnly={readOnly}
          limits={DEFAULT_LIMITS}
        />
      </section>
      <section className={styles.section} aria-labelledby={stepsId} onFocus={onStepsFocus}>
        <h3 id={stepsId} className={styles.heading}>
          {t('wl.ruleeditor.steps.title')}
        </h3>
        {scriptRule ? null : (
          <p className={styles.hint}>{t('wl.ruleeditor.steps.phases', { before: split.before, after: split.after })}</p>
        )}
        {scriptRule && scriptStep !== undefined ? (
          <ScriptStepEditor
            ref={scriptRef}
            label={t('wl.ruleeditor.script.label')}
            value={scriptText}
            fullHeight
            readOnly={readOnly}
            onChange={(source) => {
              state.setSteps([setStepField(scriptStep, 'source', source)]);
            }}
          />
        ) : (
          <StepList
            ref={listRef}
            steps={draft.steps}
            catalogue={RULE_STEP_CATALOGUE}
            context={context}
            onChange={state.setSteps}
            errors={stepErrors}
            readOnly={readOnly}
            highlightedStepId={highlightedStepId}
            onStepRemoved={onStepRemoved}
          />
        )}
        {scriptRule && server.steps.length > 0 ? (
          <ul role="alert" className={styles.problems}>
            {server.steps.map((error, index) => (
              <li key={`${error.stepId}-${index}`}>{error.message}</li>
            ))}
          </ul>
        ) : null}
        {attempted && validation.stepsMissing ? (
          <p role="alert" className={styles.fieldError}>
            {t('wl.ruleeditor.issue.stepsMissing')}
          </p>
        ) : null}
        {server.general.length === 0 ? null : (
          <ul role="alert" className={styles.problems}>
            {server.general.map((error, index) => (
              <li key={`${error.field}-${index}`}>{error.message}</li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}
