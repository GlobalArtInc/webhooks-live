import { useSyncExternalStore } from 'react';
import { PREFS_STORAGE_KEY, readStoredPrefs } from '../../prefs/theme';

type Listener = () => void;

export const DEFAULT_SINGLE_KEY_SHORTCUTS = true;

const listeners = new Set<Listener>();
let unsaved: boolean | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function readFields(raw: string | null): Record<string, unknown> {
  if (raw === null) {
    return {};
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    return isRecord(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

export function parseSingleKeyPreference(raw: string | null): boolean {
  const value = readFields(raw).singleKeyShortcuts;
  return typeof value === 'boolean' ? value : DEFAULT_SINGLE_KEY_SHORTCUTS;
}

function currentPreference(): boolean {
  return unsaved ?? parseSingleKeyPreference(readStoredPrefs());
}

function notify(): void {
  for (const listener of [...listeners]) {
    listener();
  }
}

function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function saveSingleKeyPreference(enabled: boolean): void {
  try {
    const fields = readFields(readStoredPrefs());
    window.localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify({ ...fields, singleKeyShortcuts: enabled }));
    unsaved = null;
  } catch {
    unsaved = enabled;
  }
  notify();
}

export function useSingleKeyPreference(): boolean {
  return useSyncExternalStore(subscribe, currentPreference, () => DEFAULT_SINGLE_KEY_SHORTCUTS);
}
