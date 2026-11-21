import { PREFS_KEY } from './constants';

export function parseAutoRun(raw: string | null): boolean {
  if (raw === null) {
    return true;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (failure: unknown) {
    if (failure instanceof SyntaxError) {
      return true;
    }
    throw failure;
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return true;
  }
  return Reflect.get(parsed, 'autoRun') !== false;
}

export function readAutoRun(): boolean {
  try {
    return parseAutoRun(window.localStorage.getItem(PREFS_KEY));
  } catch {
    return true;
  }
}

export function writeAutoRun(value: boolean): void {
  try {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify({ autoRun: value }));
  } catch {
    return;
  }
}
