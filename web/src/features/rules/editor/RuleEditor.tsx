import type { EditorView } from '@codemirror/view';
import { useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate } from '@tanstack/react-router';
import { useCallback, useEffect, useEffectEvent, useId, useMemo, useRef, useState, type FocusEvent } from 'react';
import { useTranslation } from 'react-i18next';
import {
  createRule,
  listRules,
  patchRule,
  rulesQueryKey,
  useSetRuleEnabled,
  withCreated,
  withRule,
} from '../../../api/rules';
import type { Rule, RuleList, RuleWriteBody } from '../../../api/rulesTypes';
import type { CapturePick } from '../../../components/CapturePicker/CapturePicker';
import { ConflictBanner } from '../../../components/ConflictBanner/ConflictBanner';
import { UndoToast } from '../../../components/UndoToast/UndoToast';
import { useViewportLayout } from '../../endpoint/useViewport';
import type { CodeEditorHandle } from '../../editor/types';
import { focusRowPart } from '../conditions/focusPart';
import type { RuleEditorSearch, RuleEditorTab } from '../ruleEditorSearch';
import { RulesToast } from '../RulesToast';
import { RuleRunsPanel } from '../runlogs/RuleRunsPanel';
import { insertStep } from '../steps/model';
import type { StepListHandle, StepRemoval } from '../steps/types';
import { RuleTester } from '../tester/RuleTester';
import type { TesterHandle } from '../tester/props';
import type { DraftRuleInput, ErrorPosition, TraceSelection } from '../tester/types';
import { BuildPanel } from './BuildPanel';
import { RULE_CATALOGUE } from './catalogue';
import { EditorHeader } from './EditorHeader';
import { EditorTabs } from './EditorTabs';
import type * as EditorDomModule from './editorDom';
import { inputOfRule } from './model';
import styles from './RuleEditor.module.css';
import { RuleEditorFooter } from './RuleEditorFooter';
import { RULE_STEP_CATALOGUE } from './stepCatalogue';
import type { FocusRequest } from './types';
import { useDirtyGuard } from './useDirtyGuard';
import { useEditorContext } from './useEditorContext';
import { useRuleDraft } from './useRuleDraft';

export interface RuleEditorProps {
  endpointId: string;
  endpointUrl: string;
  routeRuleId: string;
  rule: Rule | null;
  initial: RuleWriteBody;
  startDirty: boolean;
  scriptRule: boolean;
  canEdit: boolean;
  search: RuleEditorSearch;
}

type EditorDom = typeof EditorDomModule;

interface RuleNotice {
  id: number;
  message: string;
  tone: 'info' | 'danger';
  undo: { ruleId: string; enabled: boolean } | null;
}

const EDITOR_ROUTE = '/e/$id/settings/rules/$ruleId';
const STEP_TITLES: Readonly<Record<string, string>> = Object.fromEntries(
  RULE_STEP_CATALOGUE.map((entry) => [entry.kind, entry.title]),
);

function isSaveShortcut(event: KeyboardEvent): boolean {
  return (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's';
}

function isRunShortcut(event: KeyboardEvent): boolean {
  return event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey;
}

export function RuleEditor({
  endpointId,
  endpointUrl,
  routeRuleId,
  rule,
  initial,
  startDirty,
  scriptRule: scriptRuleAtLoad,
  canEdit,
  search,
}: RuleEditorProps) {
  const { t } = useTranslation();
  const layout = useViewportLayout();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const setEnabled = useSetRuleEnabled(endpointId);
  const baseId = useId();
  const [scriptRule] = useState(scriptRuleAtLoad);
  const creating = rule === null;
  const ruleId = rule?.id ?? null;
  const wide = layout === 'wide';
  const tab: RuleEditorTab = search.tab ?? 'build';
  const active: RuleEditorTab = wide && tab === 'test' ? 'build' : tab;
  const visible = {
    build: active === 'build',
    test: tab === 'test' || (wide && tab === 'build'),
    runs: tab === 'runs',
  };
  const pinned = search.capture ?? null;
  const pick = useMemo<CapturePick>(
    () => (pinned === null ? { mode: 'latest' } : { mode: 'pinned', captureId: pinned }),
    [pinned],
  );

  const buildRef = useRef<HTMLDivElement>(null);
  const conditionsRef = useRef<HTMLElement>(null);
  const nameRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<StepListHandle>(null);
  const scriptRef = useRef<CodeEditorHandle>(null);
  const testerRef = useRef<TesterHandle>(null);
  const domRef = useRef<EditorDom | null>(null);
  const targetRef = useRef<EditorView | null>(null);
  const pendingRef = useRef<(() => void) | null>(null);
  const noticeCount = useRef(0);
  const [hasTarget, setHasTarget] = useState(false);
  const [selectedStep, setSelectedStep] = useState<number | null>(null);
  const [conditionsSelected, setConditionsSelected] = useState(false);
  const [notice, setNotice] = useState<RuleNotice | null>(null);
  const [removal, setRemoval] = useState<StepRemoval | null>(null);

  const refresh = useCallback(async (): Promise<Rule | null> => {
    const list = await listRules(endpointId);
    queryClient.setQueryData<RuleList>(rulesQueryKey(endpointId), list);
    return list.items.find((item) => item.id === ruleId) ?? null;
  }, [endpointId, queryClient, ruleId]);

  const write = useCallback(
    async (input: RuleWriteBody): Promise<Rule> => {
      const saved = ruleId === null ? await createRule(endpointId, input) : await patchRule(endpointId, ruleId, input);
      queryClient.setQueryData<RuleList>(rulesQueryKey(endpointId), (current) => {
        if (current === undefined) {
          return current;
        }
        return ruleId === null ? withCreated(current, saved) : withRule(current, saved);
      });
      return saved;
    },
    [endpointId, queryClient, ruleId],
  );

  const readOnlyRole = !canEdit;
  const state = useRuleDraft({
    source: rule,
    initial,
    creating,
    startDirty,
    catalogue: RULE_CATALOGUE,
    readOnly: readOnlyRole,
    refresh,
    write,
  });
  const readOnly = readOnlyRole || !state.supported;
  const blocker = useDirtyGuard(state.dirty);
  const { variables, delimiters, context } = useEditorContext(endpointId, pinned, state.draft.steps, RULE_CATALOGUE);

  const rawInput = useMemo<RuleWriteBody>(() => (rule === null ? initial : inputOfRule(rule)), [rule, initial]);
  const testerInput = useMemo<DraftRuleInput>(
    () => ({ ...(state.supported ? state.input : rawInput), enabled: rule?.enabled ?? true }),
    [state.supported, state.input, rawInput, rule?.enabled],
  );
  const testerDraft = useMemo(() => ({ ruleId, input: testerInput }), [ruleId, testerInput]);

  const stepId = selectedStep === null ? null : (state.draft.steps[selectedStep]?.id ?? null);

  function setSearch(change: Partial<RuleEditorSearch>): void {
    void navigate({
      to: EDITOR_ROUTE,
      params: { id: endpointId, ruleId: routeRuleId },
      search: (previous) => ({ ...previous, ...change }),
      replace: true,
    });
  }

  function setTab(next: RuleEditorTab): void {
    setSearch({ tab: next === 'build' ? undefined : next });
  }

  function whenBuildVisible(action: () => void): void {
    if (visible.build) {
      action();
    } else {
      pendingRef.current = action;
      setTab('build');
    }
  }

  useEffect(() => {
    const pending = pendingRef.current;
    if (visible.build && pending !== null) {
      pendingRef.current = null;
      pending();
    }
  });

  useEffect(() => {
    let cancelled = false;
    void import('./editorDom').then((module) => {
      if (!cancelled) {
        domRef.current = module;
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const host = buildRef.current;
    if (host === null) {
      return undefined;
    }
    const onFocusIn = (event: Event): void => {
      const found = domRef.current?.focusedEditor(event.composedPath()) ?? null;
      if (found !== null && found.template) {
        targetRef.current = found.view;
        setHasTarget(true);
      }
    };
    host.addEventListener('focusin', onFocusIn);
    return () => {
      host.removeEventListener('focusin', onFocusIn);
    };
  }, []);

  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (isSaveShortcut(event)) {
      event.preventDefault();
      void state.save();
    } else if (isRunShortcut(event) && !wide && tab !== 'test') {
      setTab('test');
    }
  });

  useEffect(() => {
    const handler = (event: KeyboardEvent): void => {
      onKeyDown(event);
    };
    window.addEventListener('keydown', handler);
    return () => {
      window.removeEventListener('keydown', handler);
    };
  }, []);

  const applyFocus = useEffectEvent((request: FocusRequest) => {
    const { target } = request;
    whenBuildVisible(() => {
      if (target.kind === 'name') {
        nameRef.current?.focus();
      } else if (target.kind === 'row') {
        if (buildRef.current !== null) {
          focusRowPart(buildRef.current, target.id, target.part);
        }
      } else if (target.kind === 'step') {
        if (scriptRule) {
          scriptRef.current?.focus();
        } else {
          listRef.current?.focusStep(target.stepId, target.field ?? undefined);
        }
      } else {
        buildRef.current?.querySelector<HTMLElement>('[data-add-step]:not(:disabled)')?.focus();
      }
    });
  });

  useEffect(() => {
    if (state.focusRequest !== null) {
      applyFocus(state.focusRequest);
    }
  }, [state.focusRequest]);

  useEffect(() => {
    if (state.saved !== null) {
      void navigate({
        to: EDITOR_ROUTE,
        params: { id: endpointId, ruleId: state.saved.id },
        search: (previous) => ({ tab: previous.tab, capture: previous.capture, run: previous.run }),
        replace: true,
      });
    }
  }, [state.saved, navigate, endpointId]);

  useEffect(() => {
    const root = buildRef.current;
    if (root === null) {
      return;
    }
    let element: Element | null = null;
    if (stepId !== null) {
      element = root.querySelector(`[data-step-id="${CSS.escape(stepId)}"]`);
    } else if (conditionsSelected) {
      element = conditionsRef.current;
    }
    element?.scrollIntoView?.({ block: 'nearest' });
  }, [stepId, conditionsSelected]);

  function handleSelection(selection: TraceSelection | null): void {
    setSelectedStep(selection === null ? null : selection.stepIndex);
    setConditionsSelected(selection !== null && selection.stepIndex === null);
  }

  function handleStepsFocus(event: FocusEvent<HTMLElement>): void {
    const target = event.target;
    const id = target instanceof Element ? target.closest('[data-step-id]')?.getAttribute('data-step-id') : null;
    const index = state.draft.steps.findIndex((step) => step.id === id);
    if (index >= 0) {
      testerRef.current?.selectStep(index);
    }
  }

  function handleGoToError(selection: TraceSelection, position: ErrorPosition): void {
    const index = selection.stepIndex;
    whenBuildVisible(() => {
      if (index === null) {
        conditionsRef.current?.querySelector<HTMLElement>('select, input, button')?.focus();
        return;
      }
      const step = state.draft.steps[index];
      if (step === undefined) {
        return;
      }
      if (scriptRule) {
        scriptRef.current?.goTo(position.line, position.column);
        return;
      }
      const script = step.kind === 'script';
      listRef.current?.focusStep(step.id, script ? 'source' : undefined);
      const card = buildRef.current?.querySelector(`[data-step-id="${CSS.escape(step.id)}"]`) ?? null;
      if (script && card !== null) {
        void domRef.current?.goToInElement(card, position.line, position.column);
      }
    });
  }

  function insertVariable(path: string): void {
    const view = targetRef.current;
    const known = variables.find((entry) => entry.path === path);
    const text = known?.insertText ?? `${delimiters.open} ${path} ${delimiters.close}`;
    if (view === null || domRef.current?.insertAtCursor(view, text) !== true) {
      targetRef.current = null;
      setHasTarget(false);
    }
  }

  function notify(message: string, tone: RuleNotice['tone'], undo: RuleNotice['undo']): void {
    noticeCount.current += 1;
    setRemoval(null);
    setNotice({ id: noticeCount.current, message, tone, undo });
  }

  function toggleEnabled(): void {
    if (rule === null) {
      return;
    }
    const next = !rule.enabled;
    setEnabled.mutate(
      { ruleId: rule.id, enabled: next },
      {
        onSuccess: () => {
          const message = next
            ? t('wl.rules.toggle.on', { name: rule.name })
            : t('wl.rules.toggle.off', { name: rule.name });
          notify(message, 'info', { ruleId: rule.id, enabled: rule.enabled });
        },
        onError: () => {
          notify(t('wl.rules.error.toggle'), 'danger', null);
        },
      },
    );
  }

  const undo = notice?.undo ?? null;
  const nameMessages = [
    ...state.server.name.map((error) => error.message),
    ...(state.attempted && state.validation.nameMissing ? [t('wl.ruleeditor.issue.nameMissing')] : []),
    ...(state.attempted && state.validation.nameTooLong ? [t('wl.ruleeditor.issue.nameTooLong')] : []),
  ];
  const tabs: RuleEditorTab[] = wide ? ['build', 'runs'] : ['build', 'test', 'runs'];
  const panelId = (value: RuleEditorTab): string => `${baseId}-panel-${value}`;
  const tabId = (value: RuleEditorTab): string => `${baseId}-tab-${value}`;
  const split = wide && tab !== 'runs';

  return (
    <section className={styles.page} aria-label={t('wl.ruleeditor.region')}>
      <EditorHeader
        endpointId={endpointId}
        creating={creating}
        name={state.draft.name}
        nameMessages={nameMessages}
        readOnly={readOnly}
        onNameChange={state.setName}
        enabled={rule === null ? null : rule.enabled}
        toggleBusy={setEnabled.isPending || readOnlyRole}
        onToggle={toggleEnabled}
        nameRef={nameRef}
      />
      {state.conflict ? <ConflictBanner onReload={state.reload} onOverwrite={state.overwrite} /> : null}
      {state.supported ? null : (
        <p role="alert" className={styles.notice}>
          {t('wl.ruleeditor.unsupported')}
        </p>
      )}
      {state.missing ? (
        <p role="alert" className={`${styles.notice} ${styles.errorNotice}`}>
          {t('wl.ruleeditor.missing')}
        </p>
      ) : null}
      {readOnlyRole ? <p className={styles.notice}>{t('wl.ruleeditor.readOnly')}</p> : null}
      <EditorTabs tabs={tabs} value={active} onChange={setTab} panelId={panelId} tabId={tabId} />
      <div className={styles.body} data-split={split}>
        <div
          id={panelId('build')}
          ref={buildRef}
          role="tabpanel"
          aria-labelledby={tabId('build')}
          className={styles.buildColumn}
          hidden={!visible.build}
        >
          <BuildPanel
            state={state}
            readOnly={readOnly}
            scriptRule={scriptRule}
            context={context}
            variables={variables}
            highlightedStepId={stepId}
            conditionsHighlighted={conditionsSelected}
            conditionsRef={conditionsRef}
            listRef={listRef}
            scriptRef={scriptRef}
            onStepRemoved={(next) => {
              setNotice(null);
              setRemoval(next);
            }}
            onStepsFocus={handleStepsFocus}
          />
        </div>
        <div
          id={panelId('test')}
          role={wide ? undefined : 'tabpanel'}
          aria-labelledby={wide ? undefined : tabId('test')}
          className={styles.testColumn}
          hidden={!visible.test}
        >
          {search.run === undefined ? null : (
            <p className={styles.notice}>
              {t('wl.ruleeditor.recorded.testNote')}{' '}
              <Link
                to={EDITOR_ROUTE}
                params={{ id: endpointId, ruleId: routeRuleId }}
                search={{ tab: 'runs', capture: search.capture, run: search.run }}
                className={styles.link}
              >
                {t('wl.ruleeditor.recorded.view')}
              </Link>
            </p>
          )}
          <RuleTester
            ref={testerRef}
            endpointId={endpointId}
            endpointUrl={endpointUrl}
            draft={testerDraft}
            showScope={false}
            blocked={state.validation.count > 0}
            dirty={state.dirty}
            canEdit={canEdit}
            pick={pick}
            onPickChange={(next) => {
              setSearch({ capture: next.mode === 'pinned' ? next.captureId : undefined });
            }}
            stepTitles={STEP_TITLES}
            onSelectionChange={handleSelection}
            onGoToError={handleGoToError}
            onInsertVariable={hasTarget ? insertVariable : undefined}
          />
        </div>
        <div
          id={panelId('runs')}
          role="tabpanel"
          aria-labelledby={tabId('runs')}
          className={styles.runsColumn}
          hidden={!visible.runs}
        >
          {visible.runs ? (
            <>
              {search.run === undefined ? null : (
                <p className={styles.notice}>
                  {t('wl.ruleeditor.recorded.runsNote')}{' '}
                  {search.capture === undefined ? null : (
                    <Link
                      to="/e/$id/c/$captureId"
                      params={{ id: endpointId, captureId: search.capture }}
                      className={styles.link}
                    >
                      {t('wl.ruleeditor.recorded.openCapture')}
                    </Link>
                  )}
                </p>
              )}
              <RuleRunsPanel endpointId={endpointId} testerLinks />
            </>
          ) : null}
        </div>
      </div>
      <RuleEditorFooter
        readOnly={readOnly}
        dirty={state.dirty}
        saving={state.saving}
        failed={state.saveFailed}
        blocker={blocker}
        onSave={() => {
          void state.save();
        }}
        onDiscard={state.discard}
      />
      <div role="status" className={styles.announcer}>
        {state.status === null ? null : (
          <span key={state.status.serial}>
            {state.status.kind === 'saved'
              ? t('wl.ruleeditor.saved')
              : t('wl.ruleeditor.problems', { count: state.status.count })}
          </span>
        )}
      </div>
      {notice === null ? null : (
        <RulesToast
          key={notice.id}
          message={notice.message}
          tone={notice.tone}
          onUndo={
            undo === null
              ? null
              : () => {
                  setNotice(null);
                  setEnabled.mutate({ ruleId: undo.ruleId, enabled: undo.enabled });
                }
          }
          onDismiss={() => {
            setNotice(null);
          }}
        />
      )}
      {removal === null ? null : (
        <UndoToast
          message={t('wl.ruleeditor.stepRemoved')}
          onUndo={() => {
            state.setSteps(insertStep(state.draft.steps, removal.index, removal.step));
            setRemoval(null);
          }}
          onDismiss={() => {
            setRemoval(null);
          }}
        />
      )}
    </section>
  );
}
