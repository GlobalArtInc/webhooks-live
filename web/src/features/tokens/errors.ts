import { ApiError } from '../../api/client';
import type { FieldError } from '../../api/types';

export type TokenErrorKind = 'limit' | 'rate' | 'session' | 'fields' | 'generic';

export interface TokenErrorInfo {
  kind: TokenErrorKind;
  max: number;
  resetsAt: string | null;
  retryAfter: number | null;
  fields: FieldError[];
  maybeCreated: boolean;
}

const SESSION_CODES: readonly string[] = ['sign_in_required', 'unauthenticated', 'session_required'];
const MILLISECONDS_PER_SECOND = 1000;
const SERVER_ERROR_STATUS = 500;

function info(kind: TokenErrorKind, error: ApiError): TokenErrorInfo {
  return {
    kind,
    max: error.limit?.max ?? 0,
    resetsAt: error.limit?.resets_at ?? null,
    retryAfter: error.retryAfter,
    fields: error.details,
    maybeCreated: error.status >= SERVER_ERROR_STATUS,
  };
}

export function describeTokenError(error: unknown): TokenErrorInfo {
  if (!(error instanceof ApiError)) {
    return { kind: 'generic', max: 0, resetsAt: null, retryAfter: null, fields: [], maybeCreated: true };
  }
  if (error.status === 422 && error.code === 'limit_reached') {
    return info('limit', error);
  }
  if (error.status === 429 && error.code === 'quota_exceeded') {
    return info('rate', error);
  }
  if ((error.status === 401 || error.status === 403) && error.code !== null && SESSION_CODES.includes(error.code)) {
    return info('session', error);
  }
  if (error.status === 422 && error.details.length > 0) {
    return info('fields', error);
  }
  return info('generic', error);
}

export function retryTime(error: TokenErrorInfo, now: number): number | null {
  if (error.resetsAt !== null) {
    const parsed = Date.parse(error.resetsAt);
    if (!Number.isNaN(parsed)) {
      return parsed;
    }
  }
  return error.retryAfter === null ? null : now + error.retryAfter * MILLISECONDS_PER_SECOND;
}
