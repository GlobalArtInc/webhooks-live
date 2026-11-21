import type { CaptureSummary } from '../../api/types';
import { indexOfCapture, NOT_FOUND } from './listModel';

export type SeekTarget = { kind: 'load'; captureId: string } | { kind: 'next'; captureId: string } | { kind: 'last' };

export type SeekOutcome = { status: 'pending' } | { status: 'done'; captureId: string | null };

const PENDING: SeekOutcome = { status: 'pending' };

function finish(captureId: string | null): SeekOutcome {
  return { status: 'done', captureId };
}

function awaitMore(hasNextPage: boolean): SeekOutcome {
  return hasNextPage ? PENDING : finish(null);
}

export function resolveSeek(items: readonly CaptureSummary[], hasNextPage: boolean, target: SeekTarget): SeekOutcome {
  if (target.kind === 'load') {
    return indexOfCapture(items, target.captureId) === NOT_FOUND ? awaitMore(hasNextPage) : finish(null);
  }
  if (target.kind === 'next') {
    const index = indexOfCapture(items, target.captureId);
    const older = index === NOT_FOUND ? undefined : items[index + 1];
    return older === undefined ? awaitMore(hasNextPage) : finish(older.id);
  }
  if (hasNextPage) {
    return PENDING;
  }
  return finish(items.at(-1)?.id ?? null);
}
