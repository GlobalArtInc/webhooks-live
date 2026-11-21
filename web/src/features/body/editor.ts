import { html } from '@codemirror/lang-html';
import { json } from '@codemirror/lang-json';
import { xml } from '@codemirror/lang-xml';
import { foldGutter, foldKeymap, syntaxTree } from '@codemirror/language';
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
import type { CodeLanguage, EditorConfig, EditorHandle } from './types';

const NODE_CLASSES: ReadonlyMap<string, string> = new Map([
  ['PropertyName', 'cm-wl-key'],
  ['AttributeName', 'cm-wl-attr'],
  ['String', 'cm-wl-string'],
  ['AttributeValue', 'cm-wl-string'],
  ['UnquotedAttributeValue', 'cm-wl-string'],
  ['Number', 'cm-wl-number'],
  ['EntityReference', 'cm-wl-number'],
  ['CharacterReference', 'cm-wl-number'],
  ['True', 'cm-wl-literal'],
  ['False', 'cm-wl-literal'],
  ['Null', 'cm-wl-literal'],
  ['TagName', 'cm-wl-tag'],
  ['StartTag', 'cm-wl-punct'],
  ['StartCloseTag', 'cm-wl-punct'],
  ['EndTag', 'cm-wl-punct'],
  ['SelfCloseEndTag', 'cm-wl-punct'],
  ['SelfClosingEndTag', 'cm-wl-punct'],
  ['Comment', 'cm-wl-comment'],
  ['LineComment', 'cm-wl-comment'],
  ['BlockComment', 'cm-wl-comment'],
  ['ProcessingInst', 'cm-wl-meta'],
  ['DoctypeDecl', 'cm-wl-meta'],
  ['Cdata', 'cm-wl-meta'],
]);

const ERROR_CLASS = 'cm-wl-error';

function buildSyntaxDecorations(view: EditorView): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  const tree = syntaxTree(view.state);
  for (const visible of view.visibleRanges) {
    tree.iterate({
      from: visible.from,
      to: visible.to,
      enter: (node) => {
        const className = node.type.isError ? ERROR_CLASS : NODE_CLASSES.get(node.name);
        if (className !== undefined && node.to > node.from) {
          ranges.push(Decoration.mark({ class: className }).range(node.from, node.to));
        }
      },
    });
  }
  return Decoration.set(ranges, true);
}

const syntaxColours = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildSyntaxDecorations(view);
    }

    update(update: ViewUpdate): void {
      const treeChanged = syntaxTree(update.startState) !== syntaxTree(update.state);
      if (update.docChanged || update.viewportChanged || treeChanged) {
        this.decorations = buildSyntaxDecorations(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

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

const theme = EditorView.theme({
  '&': {
    color: 'var(--color-text)',
    backgroundColor: 'var(--color-surface-sunken)',
    font: 'var(--mono-data)',
    maxHeight: '36rem',
    border: '1px solid var(--color-border)',
    borderRadius: 'var(--radius-md)',
  },
  '&.cm-focused': {
    outline: '2px solid var(--color-focus)',
    outlineOffset: '2px',
  },
  '.cm-scroller': {
    font: 'inherit',
    overflow: 'auto',
  },
  '.cm-content': {
    padding: 'var(--space-2) 0',
  },
  '.cm-line': {
    padding: '0 var(--space-3)',
  },
  '.cm-gutters': {
    backgroundColor: 'var(--color-surface-sunken)',
    color: 'var(--color-text-muted)',
    borderRight: '1px solid var(--color-border)',
  },
  '.cm-lineNumbers .cm-gutterElement': {
    padding: '0 var(--space-2)',
  },
  '.cm-foldGutter .cm-gutterElement': {
    cursor: 'pointer',
    padding: '0 var(--space-1)',
  },
  '.cm-foldPlaceholder': {
    backgroundColor: 'var(--color-border)',
    color: 'var(--color-text)',
    border: 'none',
    borderRadius: 'var(--radius-sm)',
  },
  '.cm-selectionBackground, &.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground': {
    backgroundColor: 'var(--color-accent-soft)',
  },
  '.cm-wl-match': {
    backgroundColor: 'var(--color-accent-soft)',
    outline: '1px solid var(--color-accent-text)',
  },
  '.cm-wl-key': { color: 'var(--color-accent-text)' },
  '.cm-wl-attr': { color: 'var(--color-warning)' },
  '.cm-wl-string': { color: 'var(--color-success)' },
  '.cm-wl-number': { color: 'var(--color-info)' },
  '.cm-wl-literal': { color: 'var(--color-warning)' },
  '.cm-wl-tag': { color: 'var(--color-info)' },
  '.cm-wl-punct': { color: 'var(--color-text-muted)' },
  '.cm-wl-comment': { color: 'var(--color-text-muted)', fontStyle: 'italic' },
  '.cm-wl-meta': { color: 'var(--color-text-muted)' },
  '.cm-wl-error': { color: 'var(--color-danger)', textDecoration: 'underline' },
});

function languageExtension(language: CodeLanguage): Extension {
  switch (language) {
    case 'json':
      return json();
    case 'xml':
      return xml({ autoCloseTags: false });
    case 'html':
      return html({ autoCloseTags: false, matchClosingTags: false });
    case 'plain':
      return [];
  }
}

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

  const view = new EditorView({
    parent,
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
        syntaxColours,
        matchHighlights,
        theme,
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
