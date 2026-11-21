import type { RowPart } from './types';

const CONTROLS = 'input, select, textarea, button, [contenteditable="true"]';

function findControl(root: ParentNode): HTMLElement | null {
  const direct = root.querySelector<HTMLElement>(CONTROLS);
  if (direct !== null) {
    return direct;
  }
  for (const element of root.querySelectorAll('*')) {
    const inner = element.shadowRoot?.querySelector<HTMLElement>(CONTROLS) ?? null;
    if (inner !== null) {
      return inner;
    }
  }
  return null;
}

function focusControl(element: HTMLElement | null): boolean {
  if (element === null) {
    return false;
  }
  const target = element.matches(CONTROLS) ? element : findControl(element);
  if (target === null) {
    return false;
  }
  target.focus();
  return true;
}

export function focusRowPart(host: HTMLElement, id: string, part: RowPart): boolean {
  const row = host.querySelector<HTMLElement>(`[data-row-id="${id}"], [data-group-id="${id}"]`);
  if (row === null) {
    return false;
  }
  if (part !== 'row' && focusControl(row.querySelector<HTMLElement>(`[data-part="${part}"]`))) {
    return true;
  }
  return focusControl(row);
}
