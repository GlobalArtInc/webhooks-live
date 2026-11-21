import { useEffect, useMemo, useRef, useState } from 'react';
import { ApiError } from '../../../api/client';
import type { FieldError } from '../../../api/types';
import { CONFLICT_STATUSES } from './constants';
import { mapFieldErrors } from './fieldErrors';
import {
  changedNodeIds,
  conditionsKey,
  findNode,
  newGroup,
  parseSource,
  serializeConditions,
  walkNodes,
} from './model';
import type {
  ConditionCatalogue,
  ConditionsDraft,
  ConditionsInput,
  GroupDraft,
  IssueMap,
  OnMatch,
  RowIssue,
  RowPart,
  RuleConditionsSource,
  TriggerKind,
} from './types';
import { validateConditions, type ConditionsValidation } from './validation';

export type SaveConditions = (input: ConditionsInput, version: number) => Promise<RuleConditionsSource>;

export interface ConditionsDraftOptions {
  source: RuleConditionsSource;
  catalogue: ConditionCatalogue;
  save: SaveConditions;
  onConflict?: () => void;
  readOnly: boolean;
}

export type FocusTarget = { kind: 'row'; id: string; part: RowPart } | { kind: 'triggers' } | { kind: 'onMatch' };

export interface FocusRequest {
  target: FocusTarget;
  serial: number;
}

export interface StatusNote {
  kind: 'saved' | 'problems';
  count: number;
  serial: number;
}

export interface ServerProblems {
  rows: ReadonlyMap<string, readonly RowIssue[]>;
  triggers: readonly FieldError[];
  onMatch: readonly FieldError[];
  general: readonly FieldError[];
}

export interface ConditionsDraftState {
  supported: boolean;
  draft: ConditionsDraft;
  dirty: boolean;
  saving: boolean;
  saveFailed: boolean;
  conflict: boolean;
  attempted: boolean;
  validation: ConditionsValidation;
  issues: IssueMap;
  server: ServerProblems;
  focusRequest: FocusRequest | null;
  status: StatusNote | null;
  setRoot: (root: GroupDraft) => void;
  setTriggers: (triggers: TriggerKind[]) => void;
  setOnMatch: (onMatch: OnMatch) => void;
  save: () => Promise<void>;
  discard: () => void;
  reload: () => void;
  overwrite: () => void;
}

const NO_PROBLEMS: ServerProblems = { rows: new Map(), triggers: [], onMatch: [], general: [] };

function emptyDraft(): ConditionsDraft {
  return { triggers: [], root: newGroup(), onMatch: 'next' };
}

function draftOf(source: RuleConditionsSource, catalogue: ConditionCatalogue): ConditionsDraft | null {
  const parsed = parseSource(source, catalogue);
  return parsed.ok ? parsed.value : null;
}

function firstProblem(root: GroupDraft, rows: ReadonlyMap<string, readonly RowIssue[]>): FocusTarget | null {
  for (const node of walkNodes(root)) {
    const first = rows.get(node.id)?.[0];
    if (first !== undefined) {
      return { kind: 'row', id: node.id, part: first.part };
    }
  }
  return null;
}

function mergeIssues(client: IssueMap, server: ServerProblems['rows']): IssueMap {
  const merged = new Map<string, RowIssue[]>();
  for (const [id, list] of [...client, ...server]) {
    merged.set(id, [...(merged.get(id) ?? []), ...list]);
  }
  return merged;
}

export function useConditionsDraft(options: ConditionsDraftOptions): ConditionsDraftState {
  const { source, catalogue, save: write, onConflict, readOnly } = options;
  const [base, setBase] = useState(source);
  const [draft, setDraft] = useState<ConditionsDraft | null>(() => draftOf(source, catalogue));
  const [saving, setSaving] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [conflictFromSave, setConflictFromSave] = useState(false);
  const [attempted, setAttempted] = useState(false);
  const [server, setServer] = useState<ServerProblems>(NO_PROBLEMS);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const [status, setStatus] = useState<StatusNote | null>(null);
  const latest = useRef<ConditionsDraft | null>(draft);
  const busy = useRef(false);
  const serial = useRef(0);
  const [placeholder] = useState(emptyDraft);

  useEffect(() => {
    latest.current = draft;
  });

  const baseKey = useMemo(() => {
    const parsed = draftOf(base, catalogue);
    return parsed === null ? null : conditionsKey(parsed, catalogue);
  }, [base, catalogue]);
  const dirty = draft !== null && baseKey !== null && conditionsKey(draft, catalogue) !== baseKey;
  const remoteChanged = source.version > base.version;
  const conflict = (dirty && remoteChanged) || conflictFromSave;
  const working = draft ?? placeholder;
  const validation = useMemo(() => validateConditions(working, catalogue), [working, catalogue]);
  const issues = useMemo(() => mergeIssues(validation.rows, server.rows), [validation.rows, server.rows]);

  if (remoteChanged && !dirty && !saving && !conflictFromSave) {
    setBase(source);
    setDraft(draftOf(source, catalogue));
  }

  function clearProblems(): void {
    setServer(NO_PROBLEMS);
    setAttempted(false);
    setSaveFailed(false);
    setConflictFromSave(false);
  }

  function request(target: FocusTarget): void {
    serial.current += 1;
    setFocusRequest({ target, serial: serial.current });
  }

  function note(kind: StatusNote['kind'], count: number): void {
    serial.current += 1;
    setStatus({ kind, count, serial: serial.current });
  }

  function update(next: ConditionsDraft): void {
    setDraft(next);
    setSaveFailed(false);
    setStatus(null);
  }

  function setRoot(root: GroupDraft): void {
    if (draft === null || readOnly) {
      return;
    }
    const changed = new Set(changedNodeIds(draft.root, root));
    if (server.rows.size > 0 && [...server.rows.keys()].some((id) => changed.has(id))) {
      setServer({ ...server, rows: new Map([...server.rows].filter(([id]) => !changed.has(id))) });
    }
    update({ ...draft, root });
  }

  function setTriggers(triggers: TriggerKind[]): void {
    if (draft === null || readOnly) {
      return;
    }
    setServer({ ...server, triggers: [] });
    update({ ...draft, triggers });
  }

  function setOnMatch(onMatch: OnMatch): void {
    if (draft === null || readOnly) {
      return;
    }
    setServer({ ...server, onMatch: [] });
    update({ ...draft, onMatch });
  }

  function failValidation(current: ConditionsDraft): void {
    setAttempted(true);
    note('problems', validation.count);
    const target = firstProblem(current.root, validation.rows);
    if (validation.triggersMissing && target === null) {
      request({ kind: 'triggers' });
    } else if (target !== null) {
      request(target);
    }
  }

  function failServer(current: ConditionsDraft, failure: ApiError): void {
    const sent = serializeConditions(current, catalogue);
    const mapped = mapFieldErrors(failure.details, sent.paths, current.root, catalogue);
    const live = latest.current;
    const rows = new Map<string, RowIssue[]>();
    const general = [...mapped.general];
    for (const [id, list] of mapped.rows) {
      if (live !== null && findNode(live.root, id) !== undefined) {
        rows.set(id, list);
      } else {
        general.push(
          ...list.map((issue) => ({ field: 'condition', code: issue.code, message: issue.message ?? issue.code })),
        );
      }
    }
    setServer({ rows, triggers: mapped.triggers, onMatch: mapped.onMatch, general });
    note('problems', failure.details.length);
    const target = live === null ? null : firstProblem(live.root, rows);
    if (target !== null) {
      request(target);
    } else if (mapped.triggers.length > 0) {
      request({ kind: 'triggers' });
    } else if (mapped.onMatch.length > 0) {
      request({ kind: 'onMatch' });
    }
  }

  async function saveNow(): Promise<void> {
    if (draft === null || !dirty || readOnly || busy.current) {
      return;
    }
    if (validation.count > 0) {
      failValidation(draft);
      return;
    }
    busy.current = true;
    setSaving(true);
    setSaveFailed(false);
    setConflictFromSave(false);
    const sent = draft;
    try {
      const saved = await write(serializeConditions(sent, catalogue).input, base.version);
      const parsed = draftOf(saved, catalogue);
      setBase(saved);
      setServer(NO_PROBLEMS);
      setAttempted(false);
      note('saved', 0);
      if (
        parsed !== null &&
        latest.current === sent &&
        conditionsKey(parsed, catalogue) !== conditionsKey(sent, catalogue)
      ) {
        setDraft(parsed);
      }
    } catch (failure: unknown) {
      if (failure instanceof ApiError && CONFLICT_STATUSES.includes(failure.status)) {
        setConflictFromSave(true);
        onConflict?.();
      } else if (failure instanceof ApiError && failure.status === 422 && failure.details.length > 0) {
        failServer(sent, failure);
      } else {
        setSaveFailed(true);
      }
    } finally {
      busy.current = false;
      setSaving(false);
    }
  }

  function discard(): void {
    setDraft(draftOf(base, catalogue));
    clearProblems();
  }

  function reload(): void {
    setBase(source);
    setDraft(draftOf(source, catalogue));
    clearProblems();
  }

  function overwrite(): void {
    setBase(source);
    setConflictFromSave(false);
  }

  return {
    supported: draft !== null,
    draft: working,
    dirty,
    saving,
    saveFailed,
    conflict,
    attempted,
    validation,
    issues,
    server,
    focusRequest,
    status,
    setRoot,
    setTriggers,
    setOnMatch,
    save: saveNow,
    discard,
    reload,
    overwrite,
  };
}
