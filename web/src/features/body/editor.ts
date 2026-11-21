import { foldGutter, foldKeymap, syntaxHighlighting } from '@codemirror/language';
import { findNext, findPrevious, getSearchQuery, search, SearchQuery, setSearchQuery } from '@codemirror/search';
import { Compartment, EditorState, type Extension, type Range } from '@codemirror/state';
import {
  Decoration,
  drawSelection,
  EditorView,
  keymap,
  lineNumbers,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from '@codemirror/view';
import { MAX_SEARCH_HIGHLIGHTS } from './constants';
import { editorRoot, highlightStyle, languageExtension, viewerTheme } from './editorTheme';
import type { EditorConfig, EditorHandle } from './types';

const matchMark = Decoration.mark({ class: 'cm-wl-match' });

function buildMatchDecorations(view: EditorView): DecorationSet {
  const query = getSearchQuery(view.state);
  if (!query.valid) {
    return Decoration.none;
  }
  const ranges: Range<Decoration>[] = [];
  for (const visible of view.visibleRanges) {
    const cursor = query.getCursor(view.state, visible.from, visible.to);
    for (let step = cursor.next(); !step.done && ranges.length < MAX_SEARCH_HIGHLIGHTS; step = cursor.next()) {
      ranges.push(matchMark.range(step.value.from, step.value.to));
    }
  }
  return Decoration.set(ranges, true);
}

const matchHighlights = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildMatchDecorations(view);
    }

    update(update: ViewUpdate): void {
      const queryChanged = getSearchQuery(update.startState) !== getSearchQuery(update.state);
      if (update.docChanged || update.viewportChanged || queryChanged) {
        this.decorations = buildMatchDecorations(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

function wrapExtension(wrap: boolean): Extension {
  return wrap ? EditorView.lineWrapping : [];
}

function searchQueryFor(value: string): SearchQuery {
  return new SearchQuery({ search: value, caseSensitive: false, literal: true });
}

export function createEditor(parent: HTMLElement, config: EditorConfig): EditorHandle {
  const languageSlot = new Compartment();
  const wrapSlot = new Compartment();
  const labelSlot = new Compartment();

  const root = editorRoot(parent);

  const view = new EditorView({
    parent: root,
    root,
    state: EditorState.create({
      doc: config.text,
      extensions: [
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
        labelSlot.of(EditorView.contentAttributes.of({ tabindex: '0', 'aria-label': config.label })),
        lineNumbers(),
        foldGutter(),
        drawSelection(),
        keymap.of(foldKeymap),
        search(),
        syntaxHighlighting(highlightStyle),
        matchHighlights,
        viewerTheme,
        languageSlot.of(languageExtension(config.language)),
        wrapSlot.of(wrapExtension(config.wrap)),
      ],
    }),
  });

  let current = config;
  if (config.search !== '') {
    view.dispatch({ effects: setSearchQuery.of(searchQueryFor(config.search)) });
  }

  return {
    update: (next: EditorConfig): void => {
      const effects = [];
      if (next.language !== current.language) {
        effects.push(languageSlot.reconfigure(languageExtension(next.language)));
      }
      if (next.wrap !== current.wrap) {
        effects.push(wrapSlot.reconfigure(wrapExtension(next.wrap)));
      }
      if (next.label !== current.label) {
        effects.push(
          labelSlot.reconfigure(EditorView.contentAttributes.of({ tabindex: '0', 'aria-label': next.label })),
        );
      }
      if (next.search !== current.search) {
        effects.push(setSearchQuery.of(searchQueryFor(next.search)));
      }
      const replaceText = next.text !== current.text;
      view.dispatch({
        changes: replaceText ? { from: 0, to: view.state.doc.length, insert: next.text } : undefined,
        effects,
      });
      current = next;
    },
    findNext: (): void => {
      if (current.search !== '') {
        findNext(view);
      }
    },
    findPrevious: (): void => {
      if (current.search !== '') {
        findPrevious(view);
      }
    },
    destroy: (): void => {
      view.destroy();
    },
  };
}
