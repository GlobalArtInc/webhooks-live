import { PREFS_STORAGE_KEY, readStoredPrefs } from '../../prefs/theme';

export const LIST_WIDTH_DEFAULT = 380;
export const LIST_WIDTH_MIN = 300;
export const LIST_WIDTH_MAX = 560;

const LIST_WIDTH_FIELD = 'listWidth';

export function clampListWidth(value: number): number {
  return Math.min(LIST_WIDTH_MAX, Math.max(LIST_WIDTH_MIN, Math.round(value)));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function parseFields(raw: string | null): Record<string, unknown> {
  if (raw === null) {
    return {};
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    return isRecord(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

export function parseListWidth(raw: string | null): number {
  const width = parseFields(raw)[LIST_WIDTH_FIELD];
  return typeof width === 'number' && Number.isFinite(width) ? clampListWidth(width) : LIST_WIDTH_DEFAULT;
}

export function loadListWidth(): number {
  return parseListWidth(readStoredPrefs());
}

export function saveListWidth(width: number): void {
  try {
    const fields = parseFields(readStoredPrefs());
    window.localStorage.setItem(
      PREFS_STORAGE_KEY,
      JSON.stringify({ ...fields, [LIST_WIDTH_FIELD]: clampListWidth(width) }),
    );
  } catch {
    return;
  }
}
