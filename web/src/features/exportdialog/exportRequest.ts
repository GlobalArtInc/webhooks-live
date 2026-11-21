import { ApiError } from '../../api/client';
import type { CaptureExportQuery, ExportFormat } from '../../api/types';
import type { ActiveFilters } from '../list/listFilters';

export const EXPORT_MAX_CAPTURES = 500;

const DEFAULT_RETRY_SECONDS = 1;
const FALLBACK_STEM = 'webhooks-live-export';
const FILE_NAME_PATTERN = /filename="?([^";]+)"?/u;
const UNSAFE_FILE_CHARS = /[^A-Za-z0-9._-]/gu;
const MAX_FILE_NAME_LENGTH = 120;

export type ExportScope = 'selected' | 'filtered' | 'all';

export type ExportFailure = 'busy' | 'tooLarge' | 'gone' | 'failed';

export interface ExportFailed {
  kind: 'failed';
  reason: ExportFailure;
  retryAfter: number | null;
}

export function exportQueryOf(format: ExportFormat, filters: ActiveFilters): CaptureExportQuery {
  return {
    format,
    method: filters.method,
    status: filters.status,
    q: filters.q,
    since: filters.since,
    until: filters.until,
  };
}

export function fallbackFileName(format: ExportFormat): string {
  return `${FALLBACK_STEM}.${format}`;
}

export function fileNameFrom(contentDisposition: string | null, fallback: string): string {
  const match = FILE_NAME_PATTERN.exec(contentDisposition ?? '');
  const name = (match?.[1] ?? '').replace(UNSAFE_FILE_CHARS, '_').slice(0, MAX_FILE_NAME_LENGTH);
  return name === '' ? fallback : name;
}

export function failureOf(error: unknown): ExportFailed {
  if (error instanceof ApiError) {
    if (error.code === 'rate_limited' || error.status === 429) {
      return { kind: 'failed', reason: 'busy', retryAfter: error.retryAfter ?? DEFAULT_RETRY_SECONDS };
    }
    if (error.code === 'export_too_large') {
      return { kind: 'failed', reason: 'tooLarge', retryAfter: null };
    }
    if (error.status === 410 || error.code === 'endpoint_gone') {
      return { kind: 'failed', reason: 'gone', retryAfter: null };
    }
  }
  return { kind: 'failed', reason: 'failed', retryAfter: null };
}

export function selectionFitsExport(count: number): boolean {
  return count > 0 && count <= EXPORT_MAX_CAPTURES;
}
