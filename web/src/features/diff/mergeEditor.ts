import { MergeView, unifiedMergeView } from '@codemirror/merge';
import { EditorState, type Extension } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import type { MergeEditorConfig, MergeEditorHandle } from './types';

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
    position: 'relative',
    padding: '0 var(--space-3) 0 calc(var(--space-3) + 2ch)',
  },
  '.cm-gutters': {
    backgroundColor: 'var(--color-surface-sunken)',
    color: 'var(--color-text-muted)',
    borderRight: '1px solid var(--color-border)',
  },
  '.cm-insertedLine': {
    backgroundColor: 'color-mix(in srgb, var(--color-success) 16%, transparent)',
  },
  '.cm-insertedLine::before': {
    content: '"+"',
    position: 'absolute',
    left: 'var(--space-3)',
    color: 'var(--color-success)',
  },
  '.cm-deletedLine': {
    backgroundColor: 'color-mix(in srgb, var(--color-danger) 16%, transparent)',
  },
  '.cm-deletedLine::before': {
    content: '"−"',
    position: 'absolute',
    left: 'var(--space-3)',
    color: 'var(--color-danger)',
  },
  '.cm-changedLine': {
    backgroundColor: 'color-mix(in srgb, var(--color-warning) 12%, transparent)',
  },
  '.cm-changedLine::before': {
    content: '"~"',
    position: 'absolute',
    left: 'var(--space-3)',
    color: 'var(--color-warning)',
  },
  '.cm-changedText': {
    backgroundColor: 'color-mix(in srgb, var(--color-warning) 30%, transparent)',
  },
  '.cm-deletedText': {
    backgroundColor: 'color-mix(in srgb, var(--color-danger) 30%, transparent)',
  },
});

function readOnlyExtensions(label: string): Extension[] {
  return [
    EditorState.readOnly.of(true),
    EditorView.editable.of(false),
    EditorView.contentAttributes.of({ 'aria-label': label, tabindex: '0' }),
    theme,
  ];
}

export function mountMergeEditor(host: HTMLElement, config: MergeEditorConfig): MergeEditorHandle {
  const root = host.shadowRoot ?? host.attachShadow({ mode: 'open' });

  if (config.layout === 'split') {
    const view = new MergeView({
      parent: root,
      root,
      highlightChanges: true,
      gutter: true,
      a: { doc: config.original, extensions: readOnlyExtensions(config.labelOriginal) },
      b: { doc: config.modified, extensions: readOnlyExtensions(config.labelModified) },
    });
    return { destroy: () => view.destroy() };
  }

  const view = new EditorView({
    parent: root,
    root,
    state: EditorState.create({
      doc: config.modified,
      extensions: [
        ...readOnlyExtensions(config.labelUnified),
        unifiedMergeView({
          original: config.original,
          highlightChanges: true,
          gutter: true,
          mergeControls: false,
          syntaxHighlightDeletions: false,
        }),
      ],
    }),
  });
  return { destroy: () => view.destroy() };
}
