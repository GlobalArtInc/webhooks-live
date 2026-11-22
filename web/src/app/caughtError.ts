import { ApiError } from '../api/client';
import { stateKindForError } from '../components/StatePage/stateKind';

export interface CaughtErrorInfo {
  componentStack?: string | undefined;
}

export function isHandledError(error: unknown): boolean {
  return error instanceof ApiError && stateKindForError(error) !== 'error';
}

export function logCaughtError(error: unknown, info: CaughtErrorInfo): void {
  if (isHandledError(error)) {
    return;
  }
  console.error(error, info.componentStack);
}
