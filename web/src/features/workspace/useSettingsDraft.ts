import { useBlocker, type RegisteredRouter } from '@tanstack/react-router';
import { useCallback, useEffect, useState } from 'react';

export interface SettingsDraftOptions<TDraft, TSource> {
  source: TSource | undefined;
  toDraft: (source: TSource) => TDraft;
  serialize: (draft: TDraft) => string;
  refresh: () => Promise<TSource | undefined>;
  write: (draft: TDraft) => Promise<TSource>;
  validate?: (draft: TDraft) => boolean;
  readOnly?: boolean;
}

export type DraftBlocker = ReturnType<typeof useBlocker<RegisteredRouter, true>>;

export interface DraftStatus {
  dirty: boolean;
  saving: boolean;
  error: unknown;
  conflict: boolean;
  save: () => Promise<void>;
  discard: () => void;
  reload: () => void;
  overwrite: () => void;
  blocker: DraftBlocker;
}

export interface SettingsDraft<TDraft> extends DraftStatus {
  ready: boolean;
  draft: TDraft | null;
  setDraft: (draft: TDraft) => void;
  attempts: number;
}

interface ShortcutEvent {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  preventDefault: () => void;
}

const SAVE_KEY = 's';

function isSaveShortcut(event: ShortcutEvent): boolean {
  return (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === SAVE_KEY;
}

export function useSettingsDraft<TDraft, TSource>(
  options: SettingsDraftOptions<TDraft, TSource>,
): SettingsDraft<TDraft> {
  const { source, toDraft, serialize, refresh, write, validate, readOnly = false } = options;
  const [base, setBase] = useState<TSource | null>(null);
  const [draft, setDraftState] = useState<TDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [attempts, setAttempts] = useState(0);

  const baseKey = base === null ? null : serialize(toDraft(base));
  const dirty = draft !== null && baseKey !== null && serialize(draft) !== baseKey;
  const remoteKey = source === undefined ? null : serialize(toDraft(source));
  const remoteChanged = remoteKey !== null && baseKey !== null && remoteKey !== baseKey;
  const conflict = dirty && remoteChanged && !saving;

  if (source !== undefined && (base === null || (!dirty && !saving && remoteChanged))) {
    setBase(source);
    setDraftState(toDraft(source));
  }

  const setDraft = useCallback((next: TDraft) => {
    setDraftState(next);
  }, []);

  const save = useCallback(async () => {
    if (draft === null || !dirty || saving || readOnly) {
      return;
    }
    setAttempts((count) => count + 1);
    if (validate !== undefined && !validate(draft)) {
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const latest = await refresh();
      if (latest !== undefined && serialize(toDraft(latest)) !== baseKey) {
        return;
      }
      const saved = await write(draft);
      setBase(saved);
      setDraftState((current) => (current === draft ? toDraft(saved) : current));
    } catch (failure: unknown) {
      setError(failure);
    } finally {
      setSaving(false);
    }
  }, [draft, dirty, saving, readOnly, validate, refresh, serialize, toDraft, baseKey, write]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (isSaveShortcut(event)) {
        event.preventDefault();
        void save();
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [save]);

  const shouldBlock = useCallback(() => dirty, [dirty]);
  const blocker = useBlocker({
    shouldBlockFn: shouldBlock,
    enableBeforeUnload: dirty,
    withResolver: true,
  });

  function discard(): void {
    if (base !== null) {
      setDraftState(toDraft(base));
    }
  }

  function reload(): void {
    if (source !== undefined) {
      setBase(source);
      setDraftState(toDraft(source));
    }
  }

  function overwrite(): void {
    if (source !== undefined) {
      setBase(source);
    }
  }

  return {
    ready: draft !== null,
    draft,
    setDraft,
    dirty,
    saving,
    error,
    conflict,
    attempts,
    save,
    discard,
    reload,
    overwrite,
    blocker,
  };
}
