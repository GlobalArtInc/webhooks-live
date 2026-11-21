import { PREFS_STORAGE_KEY, readStoredPrefs } from '../prefs/theme';

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

export function readLanguagePreference(raw: string | null): string | null {
  const language = parseFields(raw).language;
  return typeof language === 'string' ? language : null;
}

export function loadLanguagePreference(): string | null {
  return readLanguagePreference(readStoredPrefs());
}

export function saveLanguagePreference(language: string): void {
  try {
    const fields = parseFields(readStoredPrefs());
    window.localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify({ ...fields, language }));
  } catch {
    return;
  }
}
