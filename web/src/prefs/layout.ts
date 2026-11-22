import { useSyncExternalStore } from 'react';
import { parsePrefsFields, readStoredPrefs, updatePrefsFields } from './theme';

export const RAIL_VALUES = ['expanded', 'collapsed'] as const;

export type RailPreference = (typeof RAIL_VALUES)[number];

export const BODY_VIEW_MODES = ['pretty', 'raw'] as const;

export type BodyViewModePreference = (typeof BODY_VIEW_MODES)[number];

export interface BodyViewPreference {
  mode: BodyViewModePreference;
  wrap: boolean;
}

export const DEFAULT_RAIL: RailPreference = 'expanded';

export const DEFAULT_BODY_VIEW: BodyViewPreference = { mode: 'pretty', wrap: false };

export const MAX_CLOSED_GROUPS = 100;

const NO_GROUPS: readonly string[] = [];

type LayoutListener = () => void;

const listeners = new Set<LayoutListener>();

interface BodyViewSnapshot {
  raw: string | null;
  value: BodyViewPreference;
}

interface GroupsSnapshot {
  raw: string | null;
  value: readonly string[];
}

let bodyViewSnapshot: BodyViewSnapshot | null = null;
let groupsSnapshot: GroupsSnapshot | null = null;

function isOneOf<Value extends string>(values: readonly Value[], value: unknown): value is Value {
  return values.some((candidate) => candidate === value);
}

function subscribe(listener: LayoutListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function notify(): void {
  for (const listener of [...listeners]) {
    listener();
  }
}

export function parseRail(raw: string | null): RailPreference {
  const value = parsePrefsFields(raw).rail;
  return isOneOf(RAIL_VALUES, value) ? value : DEFAULT_RAIL;
}

export function parseBodyView(raw: string | null): BodyViewPreference {
  const fields = parsePrefsFields(raw);
  return {
    mode: isOneOf(BODY_VIEW_MODES, fields.bodyMode) ? fields.bodyMode : DEFAULT_BODY_VIEW.mode,
    wrap: typeof fields.bodyWrap === 'boolean' ? fields.bodyWrap : DEFAULT_BODY_VIEW.wrap,
  };
}

export function parseClosedGroups(raw: string | null): readonly string[] {
  const value = parsePrefsFields(raw).railGroupsClosed;
  if (!Array.isArray(value)) {
    return NO_GROUPS;
  }
  const keys = value.filter((entry): entry is string => typeof entry === 'string' && entry !== '');
  return keys.slice(0, MAX_CLOSED_GROUPS);
}

export function saveRail(rail: RailPreference): void {
  updatePrefsFields({ rail });
  notify();
}

export function saveBodyView(change: Partial<BodyViewPreference>): void {
  const next = { ...parseBodyView(readStoredPrefs()), ...change };
  updatePrefsFields({ bodyMode: next.mode, bodyWrap: next.wrap });
  notify();
}

export function toggleBodyMode(): void {
  const current = parseBodyView(readStoredPrefs());
  saveBodyView({ mode: current.mode === 'pretty' ? 'raw' : 'pretty' });
}

export function toggleBodyWrap(): void {
  saveBodyView({ wrap: !parseBodyView(readStoredPrefs()).wrap });
}

export function saveGroupOpen(key: string, open: boolean): void {
  const closed = parseClosedGroups(readStoredPrefs()).filter((entry) => entry !== key);
  const next = open ? closed : [...closed, key].slice(-MAX_CLOSED_GROUPS);
  updatePrefsFields({ railGroupsClosed: next });
  notify();
}

export function resetLayoutPrefs(): void {
  updatePrefsFields({ rail: undefined, bodyMode: undefined, bodyWrap: undefined, railGroupsClosed: undefined });
  notify();
}

export function useRailPreference(): RailPreference {
  return useSyncExternalStore(
    subscribe,
    () => parseRail(readStoredPrefs()),
    () => DEFAULT_RAIL,
  );
}

function currentBodyView(): BodyViewPreference {
  const raw = readStoredPrefs();
  if (bodyViewSnapshot === null || bodyViewSnapshot.raw !== raw) {
    bodyViewSnapshot = { raw, value: parseBodyView(raw) };
  }
  return bodyViewSnapshot.value;
}

function currentGroups(): readonly string[] {
  const raw = readStoredPrefs();
  if (groupsSnapshot === null || groupsSnapshot.raw !== raw) {
    groupsSnapshot = { raw, value: parseClosedGroups(raw) };
  }
  return groupsSnapshot.value;
}

export function useBodyViewPreference(): BodyViewPreference {
  return useSyncExternalStore(subscribe, currentBodyView, () => DEFAULT_BODY_VIEW);
}

export function useClosedGroups(): readonly string[] {
  return useSyncExternalStore(subscribe, currentGroups, () => NO_GROUPS);
}
