import type { CaptureListFilters } from '../../api/types';

export const METHOD_OPTIONS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'] as const;
export type MethodOption = (typeof METHOD_OPTIONS)[number];

export const KIND_OPTIONS = ['http', 'email', 'dns'] as const;
export type KindOption = (typeof KIND_OPTIONS)[number];

export const STATUS_OPTIONS = ['1xx', '2xx', '3xx', '4xx', '5xx', 'none'] as const;
export type StatusOption = (typeof STATUS_OPTIONS)[number];

export const RANGE_OPTIONS = ['15m', '1h', '24h', '7d'] as const;
export type RangeOption = (typeof RANGE_OPTIONS)[number];

export const MAX_TEXT_LENGTH = 200;

const MINUTE_MS = 60 * 1000;
const HOUR_MS = 60 * MINUTE_MS;
const RANGE_MS: Readonly<Record<RangeOption, number>> = {
  '15m': 15 * MINUTE_MS,
  '1h': HOUR_MS,
  '24h': 24 * HOUR_MS,
  '7d': 7 * 24 * HOUR_MS,
};

export interface CaptureSearch {
  q?: string;
  method?: string;
  kind?: string;
  status?: string;
  range?: RangeOption;
  since?: string;
}

export type CaptureSearchPatch = Partial<CaptureSearch>;

function isMethodOption(token: string): token is MethodOption {
  return (METHOD_OPTIONS as readonly string[]).includes(token);
}

function isKindOption(token: string): token is KindOption {
  return (KIND_OPTIONS as readonly string[]).includes(token);
}

function isStatusOption(token: string): token is StatusOption {
  return (STATUS_OPTIONS as readonly string[]).includes(token);
}

export function isRangeOption(value: unknown): value is RangeOption {
  return typeof value === 'string' && (RANGE_OPTIONS as readonly string[]).includes(value);
}

function textOf(value: unknown): string | undefined {
  const text = typeof value === 'number' ? String(value) : value;
  if (typeof text !== 'string') {
    return undefined;
  }
  const trimmed = text.trim().slice(0, MAX_TEXT_LENGTH).trim();
  return trimmed === '' ? undefined : trimmed;
}

function tokensOf<T extends string>(
  value: unknown,
  normalise: (token: string) => string,
  isOption: (token: string) => token is T,
): T[] {
  if (typeof value !== 'string') {
    return [];
  }
  const tokens: T[] = [];
  for (const raw of value.split(',')) {
    const token = normalise(raw.trim());
    if (isOption(token) && !tokens.includes(token)) {
      tokens.push(token);
    }
  }
  return tokens;
}

function instantOf(value: unknown): string | undefined {
  if (typeof value !== 'string') {
    return undefined;
  }
  const time = Date.parse(value);
  return Number.isNaN(time) ? undefined : new Date(time).toISOString();
}

export function validateCaptureSearch(search: Record<string, unknown>): CaptureSearch {
  const result: CaptureSearch = {};
  const q = textOf(search.q);
  if (q !== undefined) {
    result.q = q;
  }
  const methods = tokensOf(search.method, (token) => token.toUpperCase(), isMethodOption);
  if (methods.length > 0) {
    result.method = methods.join(',');
  }
  const kinds = tokensOf(search.kind, (token) => token.toLowerCase(), isKindOption);
  if (kinds.length > 0) {
    result.kind = kinds.join(',');
  }
  const statuses = tokensOf(search.status, (token) => token.toLowerCase(), isStatusOption);
  if (statuses.length > 0) {
    result.status = statuses.join(',');
  }
  const since = instantOf(search.since);
  if (since !== undefined) {
    result.since = since;
    if (isRangeOption(search.range)) {
      result.range = search.range;
    }
  }
  return result;
}

export function listFiltersOf(search: CaptureSearch): CaptureListFilters {
  const filters: CaptureListFilters = {};
  if (search.method !== undefined) {
    filters.method = search.method;
  }
  if (search.kind !== undefined) {
    filters.kind = search.kind;
  }
  if (search.status !== undefined) {
    filters.status = search.status;
  }
  if (search.q !== undefined) {
    filters.q = search.q;
  }
  if (search.since !== undefined) {
    filters.since = search.since;
  }
  return filters;
}

export function rangeSince(range: RangeOption, now: number): string {
  return new Date(now - RANGE_MS[range]).toISOString();
}

export function toggleToken(list: string | undefined, token: string): string | undefined {
  const tokens = list === undefined ? [] : list.split(',');
  const next = tokens.includes(token) ? tokens.filter((item) => item !== token) : [...tokens, token];
  return next.length === 0 ? undefined : next.join(',');
}

export function activePartCount(search: CaptureSearch): number {
  const methods = search.method?.split(',').length ?? 0;
  const kinds = search.kind?.split(',').length ?? 0;
  const statuses = search.status?.split(',').length ?? 0;
  return methods + kinds + statuses + (search.range === undefined ? 0 : 1);
}

export function hasCaptureSearch(search: CaptureSearch): boolean {
  return (
    search.q !== undefined ||
    search.method !== undefined ||
    search.kind !== undefined ||
    search.status !== undefined ||
    search.range !== undefined ||
    search.since !== undefined
  );
}
