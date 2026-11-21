export const PREFS_STORAGE_KEY = 'webhooks-live:prefs:v1';

export const THEME_PREFERENCES = ['system', 'light', 'dark'] as const;

export type ThemePreference = (typeof THEME_PREFERENCES)[number];

export type ThemeAttribute = Exclude<ThemePreference, 'system'>;

export function isThemePreference(value: unknown): value is ThemePreference {
  return THEME_PREFERENCES.some((preference) => preference === value);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function parsePrefsFields(raw: string | null): Record<string, unknown> {
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

export function parseThemePreference(raw: string | null): ThemePreference {
  const theme = parsePrefsFields(raw).theme;
  return isThemePreference(theme) ? theme : 'system';
}

export function toThemeAttribute(preference: ThemePreference): ThemeAttribute | null {
  return preference === 'system' ? null : preference;
}

export function resolveThemeAttribute(raw: string | null): ThemeAttribute | null {
  return toThemeAttribute(parseThemePreference(raw));
}

export function readStoredPrefs(): string | null {
  try {
    return window.localStorage.getItem(PREFS_STORAGE_KEY);
  } catch {
    return null;
  }
}

export function loadThemePreference(): ThemePreference {
  return parseThemePreference(readStoredPrefs());
}

export function applyThemeAttribute(attribute: ThemeAttribute | null): void {
  if (attribute === null) {
    document.documentElement.removeAttribute('data-theme');
    return;
  }

  document.documentElement.setAttribute('data-theme', attribute);
}

function saveThemePreference(preference: ThemePreference): void {
  try {
    const fields = parsePrefsFields(readStoredPrefs());
    window.localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify({ ...fields, theme: preference }));
  } catch {
    return;
  }
}

export function applyThemePreference(preference: ThemePreference): void {
  saveThemePreference(preference);
  applyThemeAttribute(toThemeAttribute(preference));
}
