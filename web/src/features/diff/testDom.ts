export function shadowRoots(container: HTMLElement): ShadowRoot[] {
  return Array.from(container.querySelectorAll('div'))
    .map((element) => element.shadowRoot)
    .filter((root): root is ShadowRoot => root !== null);
}

export function editorText(container: HTMLElement): string {
  return shadowRoots(container)
    .map((root) => root.textContent ?? '')
    .join('\n');
}

export function editorCount(container: HTMLElement, selector: string): number {
  return shadowRoots(container).reduce((total, root) => total + root.querySelectorAll(selector).length, 0);
}
