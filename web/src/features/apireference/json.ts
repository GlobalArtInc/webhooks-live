export type JsonObject = Record<string, unknown>;

export function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export function objectOf(value: unknown): JsonObject {
  return isObject(value) ? value : {};
}

export function arrayOf(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

export function stringOf(value: unknown): string | null {
  return typeof value === 'string' ? value : null;
}

export function stringsOf(value: unknown): string[] {
  return arrayOf(value).filter((item): item is string => typeof item === 'string');
}

export function numberOf(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

export function without(source: JsonObject, ...keys: string[]): JsonObject {
  return Object.fromEntries(Object.entries(source).filter(([key]) => !keys.includes(key)));
}

export function formatValue(value: unknown, limit = 160): string {
  let text: string;
  try {
    text = JSON.stringify(value) ?? String(value);
  } catch {
    text = String(value);
  }
  return text.length > limit ? `${text.slice(0, limit - 1)}…` : text;
}
