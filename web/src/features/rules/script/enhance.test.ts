import { EditorView } from '@codemirror/view';
import { waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { attachEnhancements } from './enhance';
import { SCRIPT_META_FIXTURE } from './fixtures';
import type { EnhancementConfig } from './types';

const PLAIN: EnhancementConfig = { meta: null, fullHeight: false };

interface Mounted {
  container: HTMLElement;
  host: HTMLElement;
  view: EditorView;
}

function moduleCount(view: EditorView): number {
  return view.state.facet(EditorView.styleModule).length;
}

function mountContainer(): HTMLElement {
  const container = document.createElement('div');
  document.body.append(container);
  return container;
}

function mountView(container: HTMLElement): Mounted {
  const host = document.createElement('div');
  container.append(host);
  const shadow = host.attachShadow({ mode: 'open' });
  const view = new EditorView({ parent: shadow, root: shadow, doc: 'let a = 1;' });
  return { container, host, view };
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('attachEnhancements', () => {
  it('finds the editor inside the shadow root and layers its theme and hover docs on top', () => {
    const { container, view } = mountView(mountContainer());
    const before = moduleCount(view);

    const handle = attachEnhancements(container, { meta: SCRIPT_META_FIXTURE, fullHeight: false });

    expect(moduleCount(view)).toBeGreaterThan(before);
    handle.destroy();
    view.destroy();
  });

  it('adds the full-height theme only while the variant is on', () => {
    const { container, view } = mountView(mountContainer());
    const handle = attachEnhancements(container, PLAIN);
    const plain = moduleCount(view);

    handle.update({ meta: null, fullHeight: true });
    expect(moduleCount(view)).toBe(plain + 1);

    handle.update({ meta: null, fullHeight: false });
    expect(moduleCount(view)).toBe(plain);
    handle.destroy();
    view.destroy();
  });

  it('waits for an editor that is created later and attaches once it appears', async () => {
    const container = mountContainer();
    const handle = attachEnhancements(container, PLAIN);
    const { view } = mountView(container);
    const before = moduleCount(view);

    container.dataset.status = 'ready';

    await waitFor(() => {
      expect(moduleCount(view)).toBeGreaterThan(before);
    });
    handle.destroy();
    view.destroy();
  });

  it('stops touching the editor after destroy', () => {
    const { container, view } = mountView(mountContainer());
    const handle = attachEnhancements(container, PLAIN);
    const attached = moduleCount(view);

    handle.destroy();
    handle.update({ meta: null, fullHeight: true });

    expect(moduleCount(view)).toBe(attached);
    view.destroy();
  });
});
