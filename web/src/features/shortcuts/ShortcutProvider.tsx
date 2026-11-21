import { useEffect, useState, type ReactNode } from 'react';
import { ShortcutsDialog } from '../../components/ShortcutsDialog/ShortcutsDialog';
import { ShortcutContext } from './ShortcutContext';
import { findShortcut, SHORTCUTS } from './registry';
import { saveSingleKeyPreference, useSingleKeyPreference } from './prefs';
import { createShortcutStore } from './store';

const TEXT_ENTRY_TAGS: ReadonlySet<string> = new Set(['INPUT', 'TEXTAREA', 'SELECT']);
const OVERLAY_ROLES: ReadonlySet<string> = new Set(['dialog', 'alertdialog', 'menu']);
const CODE_VIEWER_CLASS = 'cm-editor';

function isIgnoredTarget(event: KeyboardEvent): boolean {
  return event.composedPath().some((node) => {
    if (!(node instanceof HTMLElement)) {
      return false;
    }
    return (
      TEXT_ENTRY_TAGS.has(node.tagName) ||
      node.isContentEditable ||
      node.classList.contains(CODE_VIEWER_CLASS) ||
      OVERLAY_ROLES.has(node.getAttribute('role') ?? '')
    );
  });
}

function hasModifier(event: KeyboardEvent): boolean {
  return event.ctrlKey || event.metaKey || event.altKey;
}

export interface ShortcutProviderProps {
  children: ReactNode;
}

export function ShortcutProvider({ children }: ShortcutProviderProps) {
  const [store] = useState(createShortcutStore);
  const [helpOpen, setHelpOpen] = useState(false);
  const singleKey = useSingleKeyPreference();

  useEffect(
    () =>
      store.setHandlers({
        help: () => {
          setHelpOpen(true);
        },
      }),
    [store],
  );

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent): void {
      if (event.defaultPrevented || hasModifier(event) || isIgnoredTarget(event)) {
        return;
      }
      const entry = findShortcut(event.key, { singleKey, isActive: store.isScopeActive });
      const handler = entry === undefined ? undefined : store.handlerOf(entry.id);
      if (handler === undefined) {
        return;
      }
      event.preventDefault();
      handler();
    }
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [store, singleKey]);

  return (
    <ShortcutContext value={store}>
      {children}
      <ShortcutsDialog
        open={helpOpen}
        shortcuts={SHORTCUTS}
        singleKey={singleKey}
        onSingleKeyChange={saveSingleKeyPreference}
        onOpenChange={setHelpOpen}
      />
    </ShortcutContext>
  );
}
