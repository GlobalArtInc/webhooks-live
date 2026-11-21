import { useContext, useEffect } from 'react';
import { ShortcutContext } from './ShortcutContext';
import type { ShortcutHandlers, ShortcutScope } from './registry';

export function useShortcutHandlers(handlers: ShortcutHandlers): void {
  const store = useContext(ShortcutContext);
  useEffect(() => store.setHandlers(handlers), [store, handlers]);
}

export function useShortcutScope(scope: ShortcutScope, active: boolean): void {
  const store = useContext(ShortcutContext);
  useEffect(() => (active ? store.enableScope(scope) : undefined), [store, scope, active]);
}

export function useOpenShortcutsHelp(): () => void {
  const store = useContext(ShortcutContext);
  return () => {
    store.handlerOf('help')?.();
  };
}
