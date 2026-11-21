import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useEffectEvent, useImperativeHandle, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { Rule } from '../../../api/rulesTypes';
import type { Capture, CaptureSummary } from '../../../api/types';
import type { CapturePick } from '../../../components/CapturePicker/CapturePicker';
import { shouldRetryRequest } from '../../../api/queries';
import { CAPTURE_POLL_MS } from './constants';
import { clampCursor, freshCursor, resetCursor, selectAt, stepBack, stepForward } from './cursor';
import { statusLabel, stepTitle } from './labels';
import { readAutoRun, writeAutoRun } from './prefs';
import type { RuleTesterProps, TesterHandle } from './props';
import { buildRules, dryRunRequestOf, sideEffectKinds } from './request';
import { initialRunnerState, LatestRunner, type RunnerState } from './runner';
import { buildTrace, findStepItem, selectionOf } from './trace';
import { httpTesterTransport } from './transport';
import type { Cursor, RealRunResult, RunInput, RunOutcome, TesterScope, TraceItem, TraceModel } from './types';

export interface RuleTesterModel {
  scope: TesterScope;
  setScope: (scope: TesterScope) => void;
  captures: readonly CaptureSummary[] | undefined;
  capture: Capture | undefined;
  captureId: string | null;
  pick: CapturePick;
  setPick: (pick: CapturePick) => void;
  auto: boolean;
  setAuto: (value: boolean) => void;
  runner: RunnerState<RunOutcome>;
  trace: TraceModel | null;
  cursor: Cursor;
  selected: TraceItem | null;
  stale: boolean;
  running: boolean;
  waiting: boolean;
  ready: boolean;
  canRun: boolean;
  run: () => void;
  next: () => void;
  back: () => void;
  reset: () => void;
  select: (index: number) => void;
  stepAnnouncement: string;
  runAnnouncement: string;
  sendSample: () => void;
  sendingSample: boolean;
  sampleError: unknown;
  effects: string[];
  realOpen: boolean;
  setRealOpen: (open: boolean) => void;
  realPending: boolean;
  realResult: RealRunResult | null;
  realError: unknown;
  confirmReal: () => void;
  savedRules: readonly Rule[] | undefined;
  savedRulesFailed: boolean;
  noRules: boolean;
}

interface StepNote {
  resultId: number;
  text: string;
}

function lastIndex(trace: TraceModel | null): number {
  return trace === null ? 0 : trace.items.length - 1;
}

export function useRuleTester(props: RuleTesterProps): RuleTesterModel {
  const {
    endpointId,
    endpointUrl,
    draft,
    transport = httpTesterTransport,
    defaultScope = 'rule',
    blocked = false,
    canEdit = true,
    ref,
    onSelectionChange,
    onPickChange,
    stepTitles,
  } = props;
  const { t } = useTranslation();
  const [scope, setScope] = useState<TesterScope>(defaultScope);
  const [internalPick, setInternalPick] = useState<CapturePick>({ mode: 'latest' });
  const [auto, setAutoState] = useState<boolean>(readAutoRun);
  const [cursor, setCursor] = useState<Cursor>(() => freshCursor(0));
  const [stepNote, setStepNote] = useState<StepNote | null>(null);
  const [realOpen, setRealOpen] = useState(false);
  const pick = props.pick ?? internalPick;

  const capturesQuery = useQuery({
    queryKey: ['rule-tester', endpointId, 'captures'],
    queryFn: () => transport.listCaptures(endpointId),
    refetchInterval: CAPTURE_POLL_MS,
    retry: shouldRetryRequest,
  });
  const captures = capturesQuery.data;
  const captureId = pick.mode === 'pinned' ? pick.captureId : (captures?.[0]?.id ?? null);
  const detailQuery = useQuery({
    queryKey: ['rule-tester', endpointId, 'capture', captureId],
    queryFn: () => transport.getCapture(endpointId, captureId ?? ''),
    enabled: captureId !== null,
    retry: shouldRetryRequest,
  });
  const savedQuery = useQuery({
    queryKey: ['rule-tester', endpointId, 'rules'],
    queryFn: () => transport.listRules(endpointId),
    enabled: scope === 'all',
    retry: shouldRetryRequest,
  });

  const payload = useMemo(() => buildRules(scope, draft, savedQuery.data), [scope, draft, savedQuery.data]);
  const input = useMemo<RunInput | null>(
    () =>
      payload === null || captureId === null || payload.rules.length === 0
        ? null
        : {
            request: dryRunRequestOf(captureId, payload.rules),
            described: payload.described,
            draftIndex: payload.draftIndex,
          },
    [payload, captureId],
  );
  const requestKey = useMemo(() => (input === null ? null : JSON.stringify(input.request)), [input]);
  const rulesKey = useMemo(() => (payload === null ? null : JSON.stringify(payload.rules)), [payload]);
  const ready = input !== null && canEdit;

  const [runner, setRunner] = useState<RunnerState<RunOutcome>>(() => initialRunnerState<RunOutcome>());
  const runnerRef = useRef<LatestRunner<RunInput, RunOutcome> | null>(null);
  const previousRules = useRef<string | null>(null);
  const currentInput = useEffectEvent(() => input);

  useEffect(() => {
    const instance = new LatestRunner<RunInput, RunOutcome>(
      async (value, signal) => ({
        run: await transport.dryRun(endpointId, value.request, signal),
        described: value.described,
        draftIndex: value.draftIndex,
        captureId: value.request.capture_id,
      }),
      setRunner,
    );
    runnerRef.current = instance;
    return () => {
      instance.dispose();
      runnerRef.current = null;
      previousRules.current = null;
    };
  }, [endpointId, transport]);

  useEffect(() => {
    const instance = runnerRef.current;
    const next = currentInput();
    if (instance === null) {
      return;
    }
    if (!auto || blocked || !ready || next === null || requestKey === null) {
      instance.cancelPending();
      previousRules.current = null;
      return;
    }
    const immediate = previousRules.current === null || previousRules.current === rulesKey;
    previousRules.current = rulesKey;
    instance.schedule(next, requestKey, immediate);
  }, [auto, blocked, ready, requestKey, rulesKey, endpointId, transport]);

  const run = useCallback(() => {
    if (input !== null && requestKey !== null && canEdit && !blocked) {
      runnerRef.current?.runNow(input, requestKey);
    }
  }, [input, requestKey, canEdit, blocked]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey) {
        event.preventDefault();
        run();
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [run]);

  const outcome = runner.result;
  const trace = useMemo(() => (outcome === null ? null : buildTrace(outcome.run, outcome.described)), [outcome]);
  const last = lastIndex(trace);
  const view = useMemo(
    () =>
      cursor.resultId === runner.resultId
        ? cursor
        : runner.manual
          ? freshCursor(runner.resultId)
          : clampCursor(cursor, runner.resultId, last),
    [cursor, runner.resultId, runner.manual, last],
  );
  const selected = trace?.items[view.selected] ?? null;
  const selectedId = selected?.id ?? null;

  const notifySelection = useEffectEvent((item: TraceItem | null) => {
    onSelectionChange?.(item === null ? null : selectionOf(item));
  });
  useEffect(() => {
    notifySelection(trace?.items.find((item) => item.id === selectedId) ?? null);
  }, [selectedId, trace]);

  const noteStep = useCallback(
    (nextCursor: Cursor) => {
      const item = trace?.items[nextCursor.selected];
      setCursor(nextCursor);
      if (item !== undefined) {
        setStepNote({
          resultId: nextCursor.resultId,
          text: t('wl.tester.announce.step', {
            position: nextCursor.selected + 1,
            total: trace?.items.length ?? 0,
            title: item.type === 'rule' ? t('wl.tester.conditions.title') : stepTitle(item.stepKind, stepTitles),
            status: statusLabel(item.status, t),
          }),
        });
      }
    },
    [trace, t, stepTitles],
  );
  const next = useCallback(() => {
    noteStep(stepForward(view, last));
  }, [noteStep, view, last]);
  const back = useCallback(() => {
    noteStep(stepBack(view));
  }, [noteStep, view]);
  const reset = useCallback(() => {
    noteStep(resetCursor(view));
  }, [noteStep, view]);
  const select = useCallback(
    (index: number) => {
      setCursor(selectAt(view, index, last));
    },
    [view, last],
  );

  useImperativeHandle(
    ref,
    (): TesterHandle => ({
      run,
      selectStep: (stepIndex) => {
        if (trace === null || stepIndex === null) {
          return;
        }
        const item = findStepItem(trace, stepIndex, outcome?.draftIndex ?? 0);
        if (item !== undefined) {
          setCursor(selectAt(view, trace.items.indexOf(item), last));
        }
      },
    }),
    [run, trace, outcome, view, last],
  );

  const setPick = useCallback(
    (value: CapturePick) => {
      setInternalPick(value);
      onPickChange?.(value);
    },
    [onPickChange],
  );
  const setAuto = useCallback((value: boolean) => {
    writeAutoRun(value);
    setAutoState(value);
  }, []);

  const realMutation = useMutation({
    mutationFn: async (): Promise<RealRunResult> => {
      const capture = detailQuery.data;
      if (capture === undefined) {
        throw new Error('The capture is not loaded');
      }
      return transport.sendReal(endpointId, endpointUrl, capture);
    },
  });
  const confirmReal = useCallback(() => {
    setRealOpen(false);
    realMutation.mutate();
  }, [realMutation]);

  const queryClient = useQueryClient();
  const sampleMutation = useMutation({
    mutationFn: () => transport.sendSample(endpointId, endpointUrl),
    onSuccess: async (createdId) => {
      await queryClient.invalidateQueries({ queryKey: ['rule-tester', endpointId, 'captures'] });
      if (createdId !== null) {
        setPick({ mode: 'pinned', captureId: createdId });
      }
    },
  });
  const sendSample = useCallback(() => {
    sampleMutation.mutate();
  }, [sampleMutation]);

  const runAnnouncement =
    runner.phase === 'done' && runner.manual && trace !== null && outcome !== null
      ? t('wl.tester.announce.finished', {
          matched: trace.matchedCount,
          rules: trace.groups.length,
          steps: trace.stepCount,
          simulated: trace.simulatedCount,
        })
      : '';

  return {
    scope,
    setScope,
    captures,
    capture: detailQuery.data,
    captureId,
    pick,
    setPick,
    auto,
    setAuto,
    runner,
    trace,
    cursor: view,
    selected,
    stale: outcome !== null && runner.resultKey !== requestKey,
    running: runner.phase === 'running',
    waiting: runner.phase === 'waiting',
    ready,
    canRun: ready && !blocked,
    run,
    next,
    back,
    reset,
    select,
    stepAnnouncement: stepNote !== null && stepNote.resultId === view.resultId ? stepNote.text : '',
    runAnnouncement,
    sendSample,
    sendingSample: sampleMutation.isPending,
    sampleError: sampleMutation.error,
    effects: sideEffectKinds(draft === null ? null : draft.input),
    realOpen,
    setRealOpen,
    realPending: realMutation.isPending,
    realResult: realMutation.data ?? null,
    realError: realMutation.error,
    confirmReal,
    savedRules: savedQuery.data,
    savedRulesFailed: savedQuery.isError,
    noRules: scope === 'all' && savedQuery.data !== undefined && payload !== null && payload.rules.length === 0,
  };
}
