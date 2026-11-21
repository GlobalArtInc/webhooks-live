import { useSyncExternalStore } from 'react';
import { PREFS_STORAGE_KEY, readStoredPrefs } from '../../prefs/theme';
import { isFollowing, reduceFollow, type FollowAction, type FollowState } from './follow';

type Listener = () => void;

export interface LivePrefs {
  follow: boolean;
  announce: boolean;
}

interface PrefsSnapshot {
  raw: string | null;
  prefs: LivePrefs;
}

export interface FollowControls {
  state: FollowState;
  following: boolean;
  dispatch: (action: FollowAction) => void;
}

export const DEFAULT_LIVE_PREFS: LivePrefs = { follow: false, announce: true };

const DEFAULT_FOLLOW: FollowState = { enabled: false, paused: false };

const listeners = new Set<Listener>();
let paused = false;
let prefsSnapshot: PrefsSnapshot | null = null;
let followSnapshot: FollowState | null = null;

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

export function parseLivePrefs(raw: string | null): LivePrefs {
  const fields = readFields(raw);
  return {
    follow: typeof fields.follow === 'boolean' ? fields.follow : DEFAULT_LIVE_PREFS.follow,
    announce: typeof fields.announce === 'boolean' ? fields.announce : DEFAULT_LIVE_PREFS.announce,
  };
}

function currentPrefs(): LivePrefs {
  const raw = readStoredPrefs();
  if (prefsSnapshot === null || prefsSnapshot.raw !== raw) {
    prefsSnapshot = { raw, prefs: parseLivePrefs(raw) };
  }
  return prefsSnapshot.prefs;
}

function notify(): void {
  for (const listener of [...listeners]) {
    listener();
  }
}

export function saveLivePrefs(change: Partial<LivePrefs>): void {
  try {
    const fields = readFields(readStoredPrefs());
    window.localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify({ ...fields, ...change }));
  } catch {
    prefsSnapshot = { raw: null, prefs: { ...currentPrefs(), ...change } };
  }
  notify();
}

function subscribe(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function currentFollow(): FollowState {
  const enabled = currentPrefs().follow;
  if (followSnapshot === null || followSnapshot.enabled !== enabled || followSnapshot.paused !== paused) {
    followSnapshot = { enabled, paused };
  }
  return followSnapshot;
}

export function dispatchFollow(action: FollowAction): void {
  const before = currentFollow();
  const after = reduceFollow(before, action);
  if (after.enabled !== before.enabled) {
    paused = after.paused;
    saveLivePrefs({ follow: after.enabled });
    return;
  }
  if (after.paused !== before.paused) {
    paused = after.paused;
    notify();
  }
}

export function resetLivePrefsState(): void {
  paused = false;
  prefsSnapshot = null;
  followSnapshot = null;
  notify();
}

export function useFollowControls(): FollowControls {
  const state = useSyncExternalStore(subscribe, currentFollow, () => DEFAULT_FOLLOW);
  return { state, following: isFollowing(state), dispatch: dispatchFollow };
}

export function useLivePrefs(): LivePrefs {
  return useSyncExternalStore(subscribe, currentPrefs, () => DEFAULT_LIVE_PREFS);
}
