import { Compartment, Prec, StateEffect, type Extension } from '@codemirror/state';
import { EditorView, hoverTooltip, type Tooltip } from '@codemirror/view';
import { findHoverTarget } from './catalogue';
import type { EnhancementConfig, Enhancements, HoverEntry, ScriptLanguageMeta } from './types';

const HOVER_DELAY_MS = 300;

const enhancementTheme = EditorView.theme({
  '.cm-wl-doc': {
    display: 'grid',
    gap: 'var(--space-1)',
    maxWidth: 'min(32rem, 85vw)',
    padding: 'var(--space-2) var(--space-3)',
  },
  '.cm-wl-doc-title': {
    font: 'var(--mono-data)',
    fontWeight: '600',
    overflowWrap: 'anywhere',
  },
  '.cm-wl-doc-text': {
    font: 'var(--text-meta)',
    color: 'var(--color-text-muted)',
    overflowWrap: 'anywhere',
  },
});

const fullHeightTheme = Prec.highest(
  EditorView.theme({
    '&': {
      height: 'max(20rem, calc(100dvh - var(--script-editor-offset, 18rem)))',
      maxHeight: 'none',
    },
  }),
);

function docElement(entry: HoverEntry): HTMLElement {
  const dom = document.createElement('div');
  dom.className = 'cm-wl-doc';
  const title = document.createElement('div');
  title.className = 'cm-wl-doc-title';
  title.textContent = entry.kind === 'function' ? entry.detail : `${entry.title}: ${entry.detail}`;
  const text = document.createElement('div');
  text.className = 'cm-wl-doc-text';
  text.textContent = entry.doc;
  dom.append(title, text);
  return dom;
}

function hoverExtension(meta: ScriptLanguageMeta): Extension {
  return hoverTooltip(
    (view, position): Tooltip | null => {
      const line = view.state.doc.lineAt(position);
      const target = findHoverTarget(line.text, position - line.from, meta);
      if (target === null) {
        return null;
      }
      return {
        pos: line.from + target.from,
        end: line.from + target.to,
        above: true,
        create: () => ({ dom: docElement(target.entry) }),
      };
    },
    { hoverTime: HOVER_DELAY_MS },
  );
}

function build(config: EnhancementConfig): Extension {
  const parts: Extension[] = [enhancementTheme];
  if (config.meta !== null) {
    parts.push(hoverExtension(config.meta));
  }
  if (config.fullHeight) {
    parts.push(fullHeightTheme);
  }
  return parts;
}

function locateView(container: HTMLElement): EditorView | null {
  for (const element of container.querySelectorAll('div')) {
    const editor = element.shadowRoot?.querySelector('.cm-editor');
    if (editor instanceof HTMLElement) {
      const view = EditorView.findFromDOM(editor);
      if (view !== null) {
        return view;
      }
    }
  }
  return null;
}

export function attachEnhancements(container: HTMLElement, initial: EnhancementConfig): Enhancements {
  const slot = new Compartment();
  let current = initial;
  let view: EditorView | null = null;
  let installed = false;
  let observer: MutationObserver | null = null;

  function apply(): void {
    if (view === null) {
      return;
    }
    if (installed) {
      view.dispatch({ effects: slot.reconfigure(build(current)) });
    } else {
      view.dispatch({ effects: StateEffect.appendConfig.of(slot.of(build(current))) });
      installed = true;
    }
  }

  function find(): void {
    if (view !== null) {
      return;
    }
    view = locateView(container);
    if (view !== null) {
      observer?.disconnect();
      observer = null;
      apply();
    }
  }

  observer = new MutationObserver(find);
  observer.observe(container, { subtree: true, childList: true, attributes: true, attributeFilter: ['data-status'] });
  find();

  return {
    update: (config: EnhancementConfig): void => {
      current = config;
      apply();
    },
    destroy: (): void => {
      observer?.disconnect();
      observer = null;
      view = null;
    },
  };
}
