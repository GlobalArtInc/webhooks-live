import type { Extension } from '@codemirror/state';
import {
  Decoration,
  MatchDecorator,
  ViewPlugin,
  type DecorationSet,
  type EditorView,
  type ViewUpdate,
} from '@codemirror/view';
import type { TemplateDelimiters } from './types';

const tokenMark = Decoration.mark({ class: 'cm-wl-template' });

function escapePattern(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

export function templatePattern(delimiters: TemplateDelimiters): RegExp {
  return new RegExp(`${escapePattern(delimiters.open)}.*?${escapePattern(delimiters.close)}`, 'g');
}

export function templateMarks(delimiters: TemplateDelimiters): Extension {
  const matcher = new MatchDecorator({
    regexp: templatePattern(delimiters),
    decoration: () => tokenMark,
  });
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = matcher.createDeco(view);
      }

      update(update: ViewUpdate): void {
        this.decorations = matcher.updateDeco(update, this.decorations);
      }
    },
    { decorations: (plugin) => plugin.decorations },
  );
}
