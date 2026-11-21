import { ApiError } from '../../api/client';

export type StateKind = 'gone' | 'not-shared' | 'error' | 'not-found' | 'signin';

export function stateKindForError(error: unknown): StateKind {
  if (!(error instanceof ApiError)) {
    return 'error';
  }
  switch (error.status) {
    case 401:
      return error.code === 'unauthenticated' ? 'signin' : 'error';
    case 403:
      return error.code === 'forbidden' ? 'not-shared' : 'error';
    case 404:
      return 'not-found';
    case 410:
      return 'gone';
    default:
      return 'error';
  }
}
