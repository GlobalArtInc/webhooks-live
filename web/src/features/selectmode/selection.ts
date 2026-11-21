import type { CaptureSummary } from '../../api/types';

export interface SelectState {
  endpointId: string | null;
  active: boolean;
  ids: ReadonlySet<string>;
}

const NO_IDS: ReadonlySet<string> = new Set<string>();

export const EMPTY_SELECTION: SelectState = { endpointId: null, active: false, ids: NO_IDS };

export function scopedTo(state: SelectState, endpointId: string): SelectState {
  return state.endpointId === endpointId ? state : { endpointId, active: false, ids: NO_IDS };
}

export function beginSelection(state: SelectState): SelectState {
  return { ...state, active: true };
}

export function toggleCapture(state: SelectState, captureId: string): SelectState {
  const ids = new Set(state.ids);
  if (state.active && ids.has(captureId)) {
    ids.delete(captureId);
  } else {
    ids.add(captureId);
  }
  return { ...state, active: true, ids };
}

export function checkCapture(state: SelectState, captureId: string): SelectState {
  return { ...state, active: true, ids: new Set([...state.ids, captureId]) };
}

export function selectAll(state: SelectState, captureIds: readonly string[]): SelectState {
  return { ...state, active: true, ids: new Set([...state.ids, ...captureIds]) };
}

export function endSelection(state: SelectState): SelectState {
  return { ...state, active: false, ids: NO_IDS };
}

export function checkedIn(state: SelectState, items: readonly CaptureSummary[]): CaptureSummary[] {
  if (!state.active) {
    return [];
  }
  return items.filter((item) => state.ids.has(item.id));
}
