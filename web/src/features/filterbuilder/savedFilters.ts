export const MAX_SAVED_FILTERS = 20;
export const MAX_SAVED_NAME_LENGTH = 60;

const STORAGE_PREFIX = 'webhooks-live:saved-filters:v1:';

export interface SavedFilter {
  id: string;
  name: string;
  filter: string;
}

function storageKeyOf(endpointId: string): string {
  return `${STORAGE_PREFIX}${endpointId}`;
}

function isSavedFilter(value: unknown): value is SavedFilter {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  return (
    'id' in value &&
    typeof value.id === 'string' &&
    'name' in value &&
    typeof value.name === 'string' &&
    'filter' in value &&
    typeof value.filter === 'string'
  );
}

export function cleanName(text: string): string | undefined {
  const trimmed = text.trim().slice(0, MAX_SAVED_NAME_LENGTH).trim();
  return trimmed === '' ? undefined : trimmed;
}

export function parseSavedFilters(raw: string | null): SavedFilter[] {
  if (raw === null) {
    return [];
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) {
    return [];
  }
  const items: unknown[] = parsed;
  return items
    .filter(isSavedFilter)
    .filter((item) => cleanName(item.name) !== undefined)
    .slice(0, MAX_SAVED_FILTERS);
}

export function loadSavedFilters(endpointId: string): SavedFilter[] {
  try {
    return parseSavedFilters(window.localStorage.getItem(storageKeyOf(endpointId)));
  } catch {
    return [];
  }
}

export function storeSavedFilters(endpointId: string, filters: SavedFilter[]): boolean {
  try {
    window.localStorage.setItem(storageKeyOf(endpointId), JSON.stringify(filters));
    return true;
  } catch {
    return false;
  }
}

export function newSavedFilterId(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function addSavedFilter(filters: SavedFilter[], entry: SavedFilter): SavedFilter[] {
  return filters.length >= MAX_SAVED_FILTERS ? filters : [...filters, entry];
}

export function renameSavedFilter(filters: SavedFilter[], id: string, name: string): SavedFilter[] {
  return filters.map((item) => (item.id === id ? { ...item, name } : item));
}

export function deleteSavedFilter(filters: SavedFilter[], id: string): SavedFilter[] {
  return filters.filter((item) => item.id !== id);
}
