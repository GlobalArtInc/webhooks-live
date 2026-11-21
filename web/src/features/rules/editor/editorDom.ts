import { EditorSelection } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { positionToOffset } from '../../editor/positions';

const EDITOR_SELECTOR = '.cm-editor';
const TEMPLATE_KINDS: readonly string[] = ['template', 'key_value'];
const FIND_ATTEMPTS = 60;

export interface FocusedEditor {
  view: EditorView;
  template: boolean;
}

function isElement(target: EventTarget): target is HTMLElement {
  return target instanceof HTMLElement;
}

export function focusedEditor(path: readonly EventTarget[]): FocusedEditor | null {
  const elements = path.filter(isElement);
  const editor = elements.find((element) => element.classList.contains('cm-editor'));
  if (editor === undefined) {
    return null;
  }
  const view = EditorView.findFromDOM(editor);
  if (view === null) {
    return null;
  }
  const kind = elements.find((element) => element.dataset.kind !== undefined)?.dataset.kind ?? '';
  return { view, template: TEMPLATE_KINDS.includes(kind) };
}

export function insertAtCursor(view: EditorView, text: string): boolean {
  if (!view.dom.isConnected || view.state.readOnly) {
    return false;
  }
  view.dispatch(view.state.update(view.state.replaceSelection(text), { scrollIntoView: true, userEvent: 'input' }));
  view.focus();
  return true;
}

function findEditorElement(root: Element): HTMLElement | null {
  const direct = root.querySelector<HTMLElement>(EDITOR_SELECTOR);
  if (direct !== null) {
    return direct;
  }
  for (const element of root.querySelectorAll('*')) {
    const inner = element.shadowRoot?.querySelector<HTMLElement>(EDITOR_SELECTOR) ?? null;
    if (inner !== null) {
      return inner;
    }
  }
  return null;
}

function placeCursor(view: EditorView, line: number, column: number): void {
  const offset = positionToOffset(view.state.doc.toString(), line, column);
  view.dispatch({ selection: EditorSelection.cursor(offset), scrollIntoView: true });
  view.focus();
}

export function goToInElement(root: Element, line: number, column: number): Promise<boolean> {
  return new Promise((resolve) => {
    let attempts = 0;
    const attempt = (): void => {
      attempts += 1;
      const editor = findEditorElement(root);
      const view = editor === null ? null : EditorView.findFromDOM(editor);
      if (view !== null) {
        placeCursor(view, line, column);
        resolve(true);
      } else if (attempts < FIND_ATTEMPTS) {
        requestAnimationFrame(attempt);
      } else {
        resolve(false);
      }
    };
    attempt();
  });
}
