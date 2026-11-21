import { useEffect, useMemo, useRef, useState } from 'react';
import { ApiError } from '../../../api/client';
import type { Rule, RuleWriteBody } from '../../../api/rulesTypes';
import { CONFLICT_STATUSES } from '../conditions/constants';
import { changedNodeIds, findNode, newGroup } from '../conditions/model';
import type { GroupDraft, RowIssue } from '../conditions/types';
import type { RuleStep as StepDraft } from '../steps/types';
import {
  draftKey,
  draftToWire,
  firstProblem,
  firstServerTarget,
  inputKey,
  inputOfRule,
  mapServerErrors,
  parseRuleInput,
  validateRule,
} from './model';
import type {
  FocusRequest,
  FocusTarget,
  RuleCatalogue,
  RuleDraft,
  RuleValidation,
  ServerProblems,
  StatusNote,
} from './types';

export interface RuleDraftOptions {
  source: Rule | null;
  initial: RuleWriteBody;
  creating: boolean;
  startDirty: boolean;
  catalogue: RuleCatalogue;
  readOnly: boolean;
  refresh: () => Promise<Rule | null>;
  write: (input: RuleWriteBody) => Promise<Rule>;
}

export interface RuleDraftState {
  supported: boolean;
  draft: RuleDraft;
  input: RuleWriteBody;
  dirty: boolean;
  saving: boolean;
  saveFailed: boolean;
  conflict: boolean;
  missing: boolean;
  attempted: boolean;
  validation: RuleValidation;
  server: ServerProblems;
  focusRequest: FocusRequest | null;
  status: StatusNote | null;
  saved: Rule | null;
  setName: (name: string) => void;
  setRoot: (root: GroupDraft) => void;
  setSteps: (steps: StepDraft[]) => void;
  save: () => Promise<void>;
  discard: () => void;
  reload: () => void;
  overwrite: () => void;
}

const NO_PROBLEMS: ServerProblems = { rows: new Map(), name: [], steps: [], general: [] };

function parsedDraft(input: RuleWriteBody, catalogue: RuleCatalogue): RuleDraft | null {
  const parsed = parseRuleInput(input, catalogue);
  return parsed.ok ? parsed.value : null;
}

export function useRuleDraft(options: RuleDraftOptions): RuleDraftState {
  const { source, initial, creating, startDirty, catalogue, readOnly, refresh, write } = options;
  const [base, setBase] = useState<RuleWriteBody>(() => (source === null ? initial : inputOfRule(source)));
  const [draft, setDraft] = useState<RuleDraft | null>(() => parsedDraft(base, catalogue));
  const [saving, setSaving] = useState(false);
  const [saveFailed, setSaveFailed] = useState(false);
  const [conflictFromSave, setConflictFromSave] = useState(false);
  const [remoteSnapshot, setRemoteSnapshot] = useState<Rule | null>(null);
  const [missing, setMissing] = useState(false);
  const [attempted, setAttempted] = useState(false);
  const [server, setServer] = useState<ServerProblems>(NO_PROBLEMS);
  const [focusRequest, setFocusRequest] = useState<FocusRequest | null>(null);
  const [status, setStatus] = useState<StatusNote | null>(null);
  const [saved, setSaved] = useState<Rule | null>(null);
  const [placeholder] = useState<RuleDraft>(() => ({ name: '', root: newGroup(), steps: [] }));
  const latest = useRef<RuleDraft | null>(draft);
  const busy = useRef(false);
  const serial = useRef(0);

  useEffect(() => {
    latest.current = draft;
  });

  const baseKey = useMemo(() => inputKey(base, catalogue), [base, catalogue]);
  const remoteInput = useMemo(() => (source === null ? null : inputOfRule(source)), [source]);
  const remoteKey = useMemo(
    () => (remoteInput === null ? null : inputKey(remoteInput, catalogue)),
    [remoteInput, catalogue],
  );
  const draftChanged = draft !== null && draftKey(draft, catalogue) !== baseKey;
  const dirty = draft !== null && saved === null && (draftChanged || (creating && startDirty));
  const remoteChanged = !creating && remoteKey !== null && remoteKey !== baseKey;
  const conflict = (dirty && remoteChanged && !saving) || conflictFromSave;
  const working = draft ?? placeholder;
  const validation = useMemo(() => validateRule(working, catalogue), [working, catalogue]);
  const input = useMemo(() => draftToWire(working, catalogue).input, [working, catalogue]);

  if (remoteChanged && remoteInput !== null && !dirty && !saving && !conflictFromSave) {
    setBase(remoteInput);
    setDraft(parsedDraft(remoteInput, catalogue));
  }

  function clearProblems(): void {
    setServer(NO_PROBLEMS);
    setAttempted(false);
    setSaveFailed(false);
    setConflictFromSave(false);
    setMissing(false);
  }

  function request(target: FocusTarget): void {
    serial.current += 1;
    setFocusRequest({ target, serial: serial.current });
  }

  function note(kind: StatusNote['kind'], count: number): void {
    serial.current += 1;
    setStatus({ kind, count, serial: serial.current });
  }

  function update(next: RuleDraft): void {
    setDraft(next);
    setSaveFailed(false);
    setStatus(null);
  }

  function setName(name: string): void {
    if (draft === null || readOnly) {
      return;
    }
    if (server.name.length > 0) {
      setServer({ ...server, name: [] });
    }
    update({ ...draft, name });
  }

  function setRoot(root: GroupDraft): void {
    if (draft === null || readOnly) {
      return;
    }
    const changed = new Set(changedNodeIds(draft.root, root));
    if ([...server.rows.keys()].some((id) => changed.has(id))) {
      setServer({ ...server, rows: new Map([...server.rows].filter(([id]) => !changed.has(id))) });
    }
    update({ ...draft, root });
  }

  function setSteps(steps: StepDraft[]): void {
    if (draft === null || readOnly) {
      return;
    }
    if (server.steps.length > 0) {
      const kept = server.steps.filter((error) => {
        const before = draft.steps.find((step) => step.id === error.stepId);
        return before !== undefined && steps.includes(before);
      });
      if (kept.length !== server.steps.length) {
        setServer({ ...server, steps: kept });
      }
    }
    update({ ...draft, steps });
  }

  function failValidation(current: RuleDraft): void {
    setAttempted(true);
    note('problems', validation.count);
    const target = firstProblem(current, validation);
    if (target !== null) {
      request(target);
    }
  }

  function failServer(sent: RuleDraft, failure: ApiError): void {
    const mapped = mapServerErrors(failure.details, draftToWire(sent, catalogue), sent, catalogue);
    const live = latest.current;
    const rows = new Map<string, readonly RowIssue[]>();
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
    const steps = mapped.steps.filter((error) => live?.steps.some((step) => step.id === error.stepId) === true);
    for (const error of mapped.steps) {
      if (!steps.includes(error)) {
        general.push({ field: 'steps', message: error.message });
      }
    }
    const problems: ServerProblems = { rows, name: mapped.name, steps, general };
    setServer(problems);
    note('problems', failure.details.length);
    const target = live === null ? null : firstServerTarget(problems, live);
    if (target !== null) {
      request(target);
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
    clearProblems();
    const sent = draft;
    try {
      if (!creating) {
        const remote = await refresh();
        if (remote === null) {
          setMissing(true);
          return;
        }
        if (inputKey(inputOfRule(remote), catalogue) !== baseKey) {
          setRemoteSnapshot(remote);
          setConflictFromSave(true);
          return;
        }
      }
      const result = await write(draftToWire(sent, catalogue).input);
      const resultInput = inputOfRule(result);
      const parsed = parsedDraft(resultInput, catalogue);
      setBase(resultInput);
      setRemoteSnapshot(null);
      setServer(NO_PROBLEMS);
      setAttempted(false);
      setSaved(creating ? result : null);
      note('saved', 0);
      if (parsed !== null && latest.current === sent && draftKey(parsed, catalogue) !== draftKey(sent, catalogue)) {
        setDraft(parsed);
      }
    } catch (failure: unknown) {
      if (failure instanceof ApiError && CONFLICT_STATUSES.includes(failure.status)) {
        setConflictFromSave(true);
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

  function adopt(target: Rule | null): void {
    if (target === null) {
      return;
    }
    const targetInput = inputOfRule(target);
    setBase(targetInput);
    setDraft(parsedDraft(targetInput, catalogue));
    setRemoteSnapshot(null);
    clearProblems();
  }

  function discard(): void {
    setDraft(parsedDraft(base, catalogue));
    clearProblems();
  }

  function reload(): void {
    adopt(remoteSnapshot ?? source);
  }

  function overwrite(): void {
    const target = remoteSnapshot ?? source;
    if (target !== null) {
      setBase(inputOfRule(target));
    }
    setRemoteSnapshot(null);
    setConflictFromSave(false);
  }

  return {
    supported: draft !== null,
    draft: working,
    input,
    dirty,
    saving,
    saveFailed,
    conflict,
    missing,
    attempted,
    validation,
    server,
    focusRequest,
    status,
    saved,
    setName,
    setRoot,
    setSteps,
    save: saveNow,
    discard,
    reload,
    overwrite,
  };
}
