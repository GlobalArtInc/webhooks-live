import type { Cursor } from './types';

export function freshCursor(resultId: number): Cursor {
  return { resultId, selected: 0, revealedTo: null };
}

export function clampCursor(cursor: Cursor, resultId: number, last: number): Cursor {
  const bound = Math.max(0, last);
  return {
    resultId,
    selected: Math.min(cursor.selected, bound),
    revealedTo: cursor.revealedTo === null ? null : Math.min(cursor.revealedTo, bound),
  };
}

export function stepForward(cursor: Cursor, last: number): Cursor {
  const next = Math.min(cursor.selected + 1, Math.max(0, last));
  return { ...cursor, selected: next, revealedTo: next };
}

export function stepBack(cursor: Cursor): Cursor {
  const previous = Math.max(cursor.selected - 1, 0);
  return { ...cursor, selected: previous, revealedTo: cursor.revealedTo === null ? null : previous };
}

export function resetCursor(cursor: Cursor): Cursor {
  return { ...cursor, selected: 0, revealedTo: 0 };
}

export function selectAt(cursor: Cursor, index: number, last: number): Cursor {
  const bound = Math.min(Math.max(0, index), Math.max(0, last));
  if (cursor.revealedTo !== null && bound > cursor.revealedTo) {
    return cursor;
  }
  return { ...cursor, selected: bound };
}

export function isRevealed(cursor: Cursor, index: number): boolean {
  return cursor.revealedTo === null || index <= cursor.revealedTo;
}
