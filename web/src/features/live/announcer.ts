import type { CaptureSummary } from '../../api/types';

export const ANNOUNCE_INTERVAL_MS = 5000;
export const ANNOUNCE_PATH_LIMIT = 60;

export interface Announcement {
  id: number;
  count: number;
  method: string;
  path: string;
}

export interface AnnouncerState {
  pending: number;
  latest: CaptureSummary | null;
  lastSpokenAt: number | null;
  counter: number;
}

export interface AnnouncerStep {
  state: AnnouncerState;
  announcement: Announcement | null;
  waitMs: number | null;
}

export const EMPTY_ANNOUNCER: AnnouncerState = { pending: 0, latest: null, lastSpokenAt: null, counter: 0 };

export function addArrivals(state: AnnouncerState, captures: readonly CaptureSummary[]): AnnouncerState {
  if (captures.length === 0) {
    return state;
  }
  const newest = captures.reduce((best, capture) => (capture.seq > best.seq ? capture : best));
  const latest = state.latest !== null && state.latest.seq > newest.seq ? state.latest : newest;
  return { ...state, pending: state.pending + captures.length, latest };
}

export function announcedPath(path: string): string {
  const shown = path === '' ? '/' : path;
  return shown.length > ANNOUNCE_PATH_LIMIT ? `${shown.slice(0, ANNOUNCE_PATH_LIMIT)}…` : shown;
}

export function stepAnnouncer(state: AnnouncerState, now: number): AnnouncerStep {
  if (state.pending === 0 || state.latest === null) {
    return { state, announcement: null, waitMs: null };
  }
  const elapsed = state.lastSpokenAt === null ? Infinity : now - state.lastSpokenAt;
  if (elapsed < ANNOUNCE_INTERVAL_MS) {
    return { state, announcement: null, waitMs: ANNOUNCE_INTERVAL_MS - elapsed };
  }
  const counter = state.counter + 1;
  const announcement: Announcement = {
    id: counter,
    count: state.pending,
    method: state.latest.method,
    path: announcedPath(state.latest.path),
  };
  return {
    state: { pending: 0, latest: null, lastSpokenAt: now, counter },
    announcement,
    waitMs: null,
  };
}
