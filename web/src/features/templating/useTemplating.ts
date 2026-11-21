import { useNavigate, useSearch } from '@tanstack/react-router';
import { useCallback, useMemo, useRef } from 'react';
import type { CaptureSummary } from '../../api/types';
import type { CapturePick } from '../../components/CapturePicker/CapturePicker';
import { variableCompletionSource, type VariableEntry } from '../../components/VariablePicker/variables';
import { DEFAULT_DELIMITERS } from '../editor/constants';
import { byteLength } from '../editor/positions';
import type { CodeEditorHandle, CompletionSource, TemplateDelimiters } from '../editor/types';
import type { ResponseSettingsDraft } from '../workspace/response/responseSettings';
import { toVariableEntries } from './catalogue';
import {
  countBySeverity,
  groupDiagnostics,
  problemItems,
  sortProblems,
  type GroupedDiagnostics,
  type ProblemItem,
} from './diagnostics';
import {
  usePreviewCaptures,
  useSendSample,
  useTemplatePreview,
  useVariableCatalogue,
  type TemplatePreview,
} from './hooks';
import { previewRequestOf } from './previewRequest';

const SECTION_ROUTE_ID = '/shell/e/$id/settings/$section';
const SECTION_PATH = '/e/$id/settings/$section';
const NO_ENTRIES: readonly VariableEntry[] = [];

export interface TemplatingOptions {
  endpointId: string;
  endpointUrl: string;
  draft: ResponseSettingsDraft | null;
  maxBodyBytes: number;
  focusField: (field: string) => void;
}

export interface TemplatingModel {
  enabled: boolean;
  entries: readonly VariableEntry[];
  catalogueLoading: boolean;
  delimiters: TemplateDelimiters;
  completions: CompletionSource | undefined;
  problems: ProblemItem[];
  grouped: GroupedDiagnostics;
  errorCount: number;
  warningCount: number;
  sizeOver: boolean;
  blocked: boolean;
  preview: TemplatePreview;
  captures: readonly CaptureSummary[] | undefined;
  pick: CapturePick;
  onPick: (pick: CapturePick) => void;
  captureLabel: string | undefined;
  hasCapture: boolean;
  sendSample: () => void;
  sendingSample: boolean;
  sampleError: unknown;
  setBodyHandle: (handle: CodeEditorHandle | null) => void;
  insertIntoBody: (text: string) => void;
  focusBody: () => void;
  registerHeader: (index: number, handle: CodeEditorHandle | null) => void;
  goTo: (item: ProblemItem) => void;
}

function pickFromSearch(preview: string | undefined): CapturePick {
  return preview === undefined ? { mode: 'latest' } : { mode: 'pinned', captureId: preview };
}

export function useTemplating({
  endpointId,
  endpointUrl,
  draft,
  maxBodyBytes,
  focusField,
}: TemplatingOptions): TemplatingModel {
  const enabled = draft?.templated === true;
  const search = useSearch({ from: SECTION_ROUTE_ID });
  const navigate = useNavigate({ from: SECTION_PATH });
  const pick = useMemo(() => pickFromSearch(search.preview), [search.preview]);
  const capturesQuery = usePreviewCaptures(endpointId, enabled);
  const captures = capturesQuery.data;
  const waiting = enabled && pick.mode === 'latest' && capturesQuery.isPending;
  const captureId = pick.mode === 'pinned' ? pick.captureId : (captures?.[0]?.id ?? null);
  const labelled = captures?.find((capture) => capture.id === captureId);
  const captureLabel = labelled === undefined ? undefined : `${labelled.method} ${labelled.path}`;

  const catalogue = useVariableCatalogue(endpointId, captureId, enabled && !waiting);
  const entries = useMemo(
    () => (catalogue.data === undefined ? NO_ENTRIES : toVariableEntries(catalogue.data.items)),
    [catalogue.data],
  );
  const completions = useMemo(() => (entries.length === 0 ? undefined : variableCompletionSource(entries)), [entries]);
  const delimiters = catalogue.data?.delimiters ?? DEFAULT_DELIMITERS;

  const request = useMemo(
    () => (draft === null || !enabled || waiting ? null : previewRequestOf(draft, captureId)),
    [draft, enabled, waiting, captureId],
  );
  const preview = useTemplatePreview({ endpointId, request, enabled: request !== null });

  const result = enabled ? preview.state.result : null;
  const problems = useMemo(() => sortProblems(problemItems(result?.diagnostics ?? [])), [result]);
  const grouped = useMemo(() => groupDiagnostics(problems), [problems]);
  const errorCount = countBySeverity(problems, 'error');
  const warningCount = countBySeverity(problems, 'warning');
  const sizeOver = enabled && draft !== null && byteLength(draft.body) > maxBodyBytes;

  const bodyHandle = useRef<CodeEditorHandle | null>(null);
  const setBodyHandle = useCallback((handle: CodeEditorHandle | null) => {
    bodyHandle.current = handle;
  }, []);
  const insertIntoBody = useCallback((text: string) => {
    bodyHandle.current?.insertAtCursor(text);
  }, []);
  const focusBody = useCallback(() => {
    bodyHandle.current?.focus();
  }, []);
  const headerHandles = useRef(new Map<number, CodeEditorHandle>());
  const registerHeader = useCallback((index: number, handle: CodeEditorHandle | null) => {
    if (handle === null) {
      headerHandles.current.delete(index);
    } else {
      headerHandles.current.set(index, handle);
    }
  }, []);

  const goTo = useCallback(
    (item: ProblemItem) => {
      const { target, diagnostic } = item;
      if (target.kind === 'body') {
        bodyHandle.current?.goTo(diagnostic.line, diagnostic.column);
      } else if (target.kind === 'header') {
        headerHandles.current.get(target.index)?.goTo(diagnostic.line, diagnostic.column);
      } else {
        focusField(target.field);
      }
    },
    [focusField],
  );

  const onPick = useCallback(
    (next: CapturePick) => {
      void navigate({
        search: (previous) => ({ ...previous, preview: next.mode === 'pinned' ? next.captureId : undefined }),
        replace: true,
      });
    },
    [navigate],
  );

  const sample = useSendSample(endpointId, endpointUrl);
  const sendSample = useCallback(() => {
    sample.mutate(undefined, {
      onSuccess: (createdId) => {
        if (createdId !== null) {
          onPick({ mode: 'pinned', captureId: createdId });
        }
      },
    });
  }, [sample, onPick]);

  return {
    enabled,
    entries,
    catalogueLoading: catalogue.isPending && enabled,
    delimiters,
    completions,
    problems,
    grouped,
    errorCount,
    warningCount,
    sizeOver,
    blocked: enabled && (errorCount > 0 || sizeOver),
    preview,
    captures,
    pick,
    onPick,
    captureLabel,
    hasCapture: captureId !== null,
    sendSample,
    sendingSample: sample.isPending,
    sampleError: sample.error,
    setBodyHandle,
    insertIntoBody,
    focusBody,
    registerHeader,
    goTo,
  };
}
