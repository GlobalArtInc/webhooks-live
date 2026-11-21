import { useCallback, useState } from 'react';

export const PANEL_PREFS_KEY = 'webhooks-live:templating-preview:v1';
export const PANEL_MIN_PERCENT = 20;
export const PANEL_MAX_PERCENT = 70;
export const PANEL_STEP_PERCENT = 5;

export interface PanelPrefs {
  hidden: boolean;
  heightPercent: number;
}

export const DEFAULT_PANEL_PREFS: PanelPrefs = { hidden: false, heightPercent: 35 };

export function clampPercent(value: number): number {
  return Math.min(PANEL_MAX_PERCENT, Math.max(PANEL_MIN_PERCENT, Math.round(value)));
}

export function parsePanelPrefs(raw: string | null): PanelPrefs {
  if (raw === null) {
    return DEFAULT_PANEL_PREFS;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return DEFAULT_PANEL_PREFS;
  }
  if (typeof parsed !== 'object' || parsed === null) {
    return DEFAULT_PANEL_PREFS;
  }
  const hidden: unknown = Reflect.get(parsed, 'hidden');
  const height: unknown = Reflect.get(parsed, 'heightPercent');
  return {
    hidden: hidden === true,
    heightPercent:
      typeof height === 'number' && Number.isFinite(height) ? clampPercent(height) : DEFAULT_PANEL_PREFS.heightPercent,
  };
}

function readStored(): PanelPrefs {
  try {
    return parsePanelPrefs(window.localStorage.getItem(PANEL_PREFS_KEY));
  } catch {
    return DEFAULT_PANEL_PREFS;
  }
}

function writeStored(prefs: PanelPrefs): void {
  try {
    window.localStorage.setItem(PANEL_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    return;
  }
}

export function usePanelPrefs(): [PanelPrefs, (next: Partial<PanelPrefs>) => void] {
  const [prefs, setPrefs] = useState<PanelPrefs>(readStored);
  const update = useCallback(
    (next: Partial<PanelPrefs>) => {
      const merged: PanelPrefs = {
        hidden: next.hidden ?? prefs.hidden,
        heightPercent: clampPercent(next.heightPercent ?? prefs.heightPercent),
      };
      writeStored(merged);
      setPrefs(merged);
    },
    [prefs],
  );
  return [prefs, update];
}
