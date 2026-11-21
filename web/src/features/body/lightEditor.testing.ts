import type { EditorConfig, EditorHandle } from './types';

const EXCERPT_CHARS = 200;

export const LENGTH_ATTRIBUTE = 'data-text-length';

export function createEditor(parent: HTMLElement, config: EditorConfig): EditorHandle {
  const content = document.createElement('pre');
  content.className = 'cm-content';
  parent.append(content);
  const show = (next: EditorConfig): void => {
    content.setAttribute(LENGTH_ATTRIBUTE, String(next.text.length));
    content.textContent = next.text.slice(0, EXCERPT_CHARS);
  };
  show(config);
  return {
    update: show,
    findNext: () => undefined,
    findPrevious: () => undefined,
    destroy: () => {
      content.remove();
    },
  };
}
