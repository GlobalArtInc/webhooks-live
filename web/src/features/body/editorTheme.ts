import { html } from '@codemirror/lang-html';
import { json } from '@codemirror/lang-json';
import { xml } from '@codemirror/lang-xml';
import { HighlightStyle } from '@codemirror/language';
import type { Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { tags } from '@lezer/highlight';
import { syntaxColour } from './syntaxPalette';
import type { CodeLanguage } from './types';

export const highlightStyle = HighlightStyle.define([
  { tag: tags.propertyName, color: syntaxColour('key') },
  { tag: tags.attributeName, color: syntaxColour('attribute') },
  { tag: [tags.string, tags.attributeValue], color: syntaxColour('string') },
  { tag: [tags.number, tags.character], color: syntaxColour('number') },
  { tag: [tags.bool, tags.null, tags.keyword], color: syntaxColour('literal') },
  { tag: tags.tagName, color: syntaxColour('tag') },
  { tag: [tags.punctuation, tags.bracket, tags.meta, tags.documentMeta], color: syntaxColour('muted') },
  { tag: tags.comment, color: syntaxColour('muted'), fontStyle: 'italic' },
  { tag: tags.invalid, color: syntaxColour('error'), textDecoration: 'underline' },
]);

export const viewerTheme = EditorView.theme({
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
});

export function languageExtension(language: CodeLanguage): Extension {
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

export function editorRoot(parent: HTMLElement): ShadowRoot {
  return parent.shadowRoot ?? parent.attachShadow({ mode: 'open' });
}
