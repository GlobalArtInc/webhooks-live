import {
  isRangeOption,
  rangeSince,
  validateCaptureSearch,
  type CaptureSearch,
  type CaptureSearchPatch,
} from '../filters/captureSearch';

const FILTER_KEYS = ['q', 'method', 'kind', 'status', 'range', 'since', 'ip', 'path', 'starred'] as const;

export function patchFor(search: CaptureSearch): CaptureSearchPatch {
  return {
    q: search.q,
    method: search.method,
    kind: search.kind,
    status: search.status,
    range: search.range,
    since: search.since,
    ip: search.ip,
    path: search.path,
    starred: search.starred,
  };
}

export function filterStringOf(search: CaptureSearch): string {
  const params = new URLSearchParams();
  for (const key of FILTER_KEYS) {
    const value = search[key];
    if (value !== undefined && !(key === 'since' && search.range !== undefined)) {
      params.set(key, String(value));
    }
  }
  return params.toString();
}

export function searchOfFilterString(filter: string, now: number): CaptureSearch {
  const raw: Record<string, string> = Object.fromEntries(new URLSearchParams(filter));
  const range = raw.range;
  if (isRangeOption(range) && raw.since === undefined) {
    raw.since = rangeSince(range, now);
  }
  return validateCaptureSearch(raw);
}

export function normalisedSearch(draft: CaptureSearch, now: number): CaptureSearch {
  return searchOfFilterString(filterStringOf(draft), now);
}
