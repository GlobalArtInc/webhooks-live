import { ApiError } from '../../api/client';
import type { ErrorLimit, SendResult } from '../../api/types';

const URL_FIELD = 'url';

export type UrlIssueCode = 'required' | 'invalid' | 'tooLong' | 'targetRefused' | 'targetUnresolvable' | 'unreachable';

export interface UrlIssue {
  code: UrlIssueCode;
  detail: string | null;
}

export type SendFailure =
  | { kind: 'url'; issue: UrlIssue }
  | { kind: 'quota'; limit: ErrorLimit | null; retryAfter: number | null }
  | { kind: 'rateLimited'; retryAfter: number | null }
  | { kind: 'timeout' }
  | { kind: 'bodyTooLarge' }
  | { kind: 'hopLimit' }
  | { kind: 'disabled' }
  | { kind: 'cancelled' }
  | { kind: 'failed' };

export type HistoryFailure = 'signIn' | 'disabled' | 'failed';

function isAbortError(error: unknown): boolean {
  return typeof error === 'object' && error !== null && 'name' in error && error.name === 'AbortError';
}

function urlFailureOf(error: ApiError): SendFailure | null {
  const field = error.details.find((item) => item.field === URL_FIELD);
  if (field === undefined) {
    return null;
  }
  switch (field.code) {
    case 'target_refused':
      return { kind: 'url', issue: { code: 'targetRefused', detail: field.message } };
    case 'target_unresolvable':
      return { kind: 'url', issue: { code: 'targetUnresolvable', detail: null } };
    case 'too_long':
      return { kind: 'url', issue: { code: 'tooLong', detail: null } };
    default:
      return { kind: 'url', issue: { code: 'invalid', detail: null } };
  }
}

export function failureOf(error: unknown): SendFailure {
  if (isAbortError(error)) {
    return { kind: 'cancelled' };
  }
  if (!(error instanceof ApiError)) {
    return { kind: 'failed' };
  }
  switch (error.status) {
    case 404:
      return error.code === 'not_found' ? { kind: 'disabled' } : { kind: 'failed' };
    case 408:
      return { kind: 'timeout' };
    case 413:
      return { kind: 'bodyTooLarge' };
    case 422:
      return error.code === 'hop_limit' ? { kind: 'hopLimit' } : (urlFailureOf(error) ?? { kind: 'failed' });
    case 429:
      return error.code === 'quota_exceeded'
        ? { kind: 'quota', limit: error.limit, retryAfter: error.retryAfter }
        : { kind: 'rateLimited', retryAfter: error.retryAfter };
    default:
      return { kind: 'failed' };
  }
}

export function failureOfResult(result: SendResult): SendFailure | null {
  if (result.status !== null || result.error === null) {
    return null;
  }
  return { kind: 'url', issue: { code: 'unreachable', detail: result.error } };
}

export function historyFailureOf(error: unknown): HistoryFailure {
  if (error instanceof ApiError) {
    if (error.status === 401) {
      return 'signIn';
    }
    if (error.status === 404) {
      return 'disabled';
    }
  }
  return 'failed';
}
