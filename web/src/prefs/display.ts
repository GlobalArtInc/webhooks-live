import { useSyncExternalStore } from 'react';
import { parsePrefsFields, readStoredPrefs, updatePrefsFields } from './theme';

export const DENSITY_VALUES = ['comfortable', 'compact'] as const;

export type Density = (typeof DENSITY_VALUES)[number];

export const TIME_DISPLAY_VALUES = ['relative', 'absolute', 'utc'] as const;

export type TimeDisplay = (typeof TIME_DISPLAY_VALUES)[number];

export const DEFAULT_DENSITY: Density = 'comfortable';

export const DEFAULT_TIME_DISPLAY: TimeDisplay = 'relative';

export const COMPACT_CLASS = 'density-compact';

type DisplayListener = () => void;

const listeners = new Set<DisplayListener>();

function isOneOf<Value extends string>(values: readonly Value[], value: unknown): value is Value {
  return values.some((candidate) => candidate === value);
}

export function parseDensity(raw: string | null): Density {
  const value = parsePrefsFields(raw).density;
  return isOneOf(DENSITY_VALUES, value) ? value : DEFAULT_DENSITY;
}

export function parseTimeDisplay(raw: string | null): TimeDisplay {
  const value = parsePrefsFields(raw).timeDisplay;
  return isOneOf(TIME_DISPLAY_VALUES, value) ? value : DEFAULT_TIME_DISPLAY;
}

export function applyDensity(density: Density): void {
  document.documentElement.classList.toggle(COMPACT_CLASS, density === 'compact');
}

function notify(): void {
  for (const listener of [...listeners]) {
    listener();
  }
}

function subscribe(listener: DisplayListener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function saveDensity(density: Density): void {
  updatePrefsFields({ density });
  applyDensity(density);
  notify();
}

export function saveTimeDisplay(timeDisplay: TimeDisplay): void {
  updatePrefsFields({ timeDisplay });
  notify();
}

export function useDensity(): Density {
  return useSyncExternalStore(
    subscribe,
    () => parseDensity(readStoredPrefs()),
    () => DEFAULT_DENSITY,
  );
}

export function useTimeDisplay(): TimeDisplay {
  return useSyncExternalStore(
    subscribe,
    () => parseTimeDisplay(readStoredPrefs()),
    () => DEFAULT_TIME_DISPLAY,
  );
}
