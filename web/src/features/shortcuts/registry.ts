import type { SourceKey } from '../../i18n/keys';

export type ShortcutScope = 'global' | 'endpoint' | 'capture';

export interface ShortcutDefinition {
  id: string;
  scope: ShortcutScope;
  keys: readonly string[];
  label: SourceKey;
}

export const SHORTCUTS = [
  { id: 'next', scope: 'endpoint', keys: ['j', 'ArrowDown'], label: 'wl.shortcuts.action.next' },
  { id: 'previous', scope: 'endpoint', keys: ['k', 'ArrowUp'], label: 'wl.shortcuts.action.previous' },
  { id: 'newest', scope: 'endpoint', keys: ['K', 'Home'], label: 'wl.shortcuts.action.newest' },
  { id: 'oldest', scope: 'endpoint', keys: ['J', 'End'], label: 'wl.shortcuts.action.oldest' },
  { id: 'toggleFollow', scope: 'endpoint', keys: ['f'], label: 'wl.shortcuts.action.toggleFollow' },
  { id: 'copyUrl', scope: 'endpoint', keys: ['c'], label: 'wl.shortcuts.action.copyUrl' },
  { id: 'settings', scope: 'endpoint', keys: ['s'], label: 'wl.shortcuts.action.settings' },
  { id: 'forwarding', scope: 'endpoint', keys: ['F'], label: 'wl.shortcuts.action.forwarding' },
  { id: 'copyCurl', scope: 'capture', keys: ['y'], label: 'wl.shortcuts.action.copyCurl' },
  { id: 'prettyRaw', scope: 'capture', keys: ['r'], label: 'wl.shortcuts.action.prettyRaw' },
  { id: 'wrap', scope: 'capture', keys: ['w'], label: 'wl.shortcuts.action.wrap' },
  { id: 'compare', scope: 'capture', keys: ['d'], label: 'wl.shortcuts.action.compare' },
  { id: 'delete', scope: 'capture', keys: ['Delete'], label: 'wl.shortcuts.action.delete' },
  { id: 'newEndpoint', scope: 'global', keys: ['n'], label: 'wl.shortcuts.action.newEndpoint' },
  { id: 'help', scope: 'global', keys: ['?'], label: 'wl.shortcuts.action.help' },
  { id: 'close', scope: 'global', keys: ['Escape'], label: 'wl.shortcuts.action.close' },
] as const satisfies readonly ShortcutDefinition[];

export type ShortcutEntry = (typeof SHORTCUTS)[number];

export type ShortcutId = ShortcutEntry['id'];

export type ShortcutHandler = () => void;

export type ShortcutHandlers = Partial<Record<ShortcutId, ShortcutHandler>>;

export const SHORTCUT_IDS: readonly ShortcutId[] = SHORTCUTS.map((entry) => entry.id);

export const SHORTCUT_SCOPES: readonly ShortcutScope[] = ['global', 'endpoint', 'capture'];

const KEY_NAMES: Readonly<Record<string, string>> = {
  ArrowDown: 'Down',
  ArrowUp: 'Up',
  Escape: 'Esc',
};

const UPPERCASE_LETTER = /^[A-Z]$/;

export function isCharacterKey(key: string): boolean {
  return [...key].length === 1;
}

export function displayKey(key: string): string {
  if (UPPERCASE_LETTER.test(key)) {
    return `Shift+${key}`;
  }
  return KEY_NAMES[key] ?? key;
}

export interface ShortcutFilter {
  singleKey: boolean;
  isActive: (scope: ShortcutScope) => boolean;
}

export function findShortcut(key: string, filter: ShortcutFilter): ShortcutEntry | undefined {
  if (!filter.singleKey && isCharacterKey(key)) {
    return undefined;
  }
  return SHORTCUTS.find((entry) => filter.isActive(entry.scope) && entry.keys.some((candidate) => candidate === key));
}
