import { PREFS_STORAGE_KEY, readStoredPrefs, updatePrefsFields } from '../../prefs/theme';

export const LIST_WIDTH_DEFAULT = 380;
export const LIST_WIDTH_DEFAULT_MAX = 520;
export const LIST_WIDTH_MIN = 300;
export const LIST_WIDTH_MAX = 560;

const LIST_WIDTH_FIELD = 'listWidth';
const LIST_WIDTH_VIEWPORT_SHARE = 0.36;

export function clampListWidth(value: number): number {
  return Math.min(LIST_WIDTH_MAX, Math.max(LIST_WIDTH_MIN, Math.round(value)));
}

export function defaultListWidth(viewportWidth: number): number {
  return Math.min(
    LIST_WIDTH_DEFAULT_MAX,
    Math.max(LIST_WIDTH_DEFAULT, clampListWidth(Math.round(viewportWidth * LIST_WIDTH_VIEWPORT_SHARE))),
  );
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
  return typeof width === 'number' && Number.isFinite(width)
    ? clampListWidth(width)
    : defaultListWidth(window.innerWidth);
}

export function loadListWidth(): number {
  return parseListWidth(readStoredPrefs());
}

export function resetListWidth(): void {
  updatePrefsFields({ [LIST_WIDTH_FIELD]: undefined });
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
