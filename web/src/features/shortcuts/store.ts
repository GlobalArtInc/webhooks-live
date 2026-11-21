import {
  SHORTCUT_IDS,
  type ShortcutHandler,
  type ShortcutHandlers,
  type ShortcutId,
  type ShortcutScope,
} from './registry';

export interface ShortcutStore {
  setHandlers: (handlers: ShortcutHandlers) => () => void;
  handlerOf: (id: ShortcutId) => ShortcutHandler | undefined;
  enableScope: (scope: ShortcutScope) => () => void;
  isScopeActive: (scope: ShortcutScope) => boolean;
}

function registerHandlers(registered: Map<ShortcutId, ShortcutHandler>, next: ShortcutHandlers): () => void {
  const owned = new Map<ShortcutId, ShortcutHandler>();
  for (const id of SHORTCUT_IDS) {
    const handler = next[id];
    if (handler !== undefined) {
      registered.set(id, handler);
      owned.set(id, handler);
    }
  }
  return () => {
    for (const [id, handler] of owned) {
      if (registered.get(id) === handler) {
        registered.delete(id);
      }
    }
  };
}

export function createShortcutStore(): ShortcutStore {
  const handlers = new Map<ShortcutId, ShortcutHandler>();
  const scopeCounts = new Map<ShortcutScope, number>();

  return {
    setHandlers: (next) => registerHandlers(handlers, next),
    handlerOf: (id) => handlers.get(id),
    enableScope: (scope) => {
      scopeCounts.set(scope, (scopeCounts.get(scope) ?? 0) + 1);
      return () => {
        const remaining = (scopeCounts.get(scope) ?? 0) - 1;
        if (remaining > 0) {
          scopeCounts.set(scope, remaining);
        } else {
          scopeCounts.delete(scope);
        }
      };
    },
    isScopeActive: (scope) => scope === 'global' || (scopeCounts.get(scope) ?? 0) > 0,
  };
}
