import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { createRef } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { editorCount, editorText } from '../../features/diff/testDom';
import type { CodeEditorHandle, EditorDiagnostic } from '../../features/editor/types';
import { createI18nInstance } from '../../i18n/setup';
import { CodeEditor, type CodeEditorProps } from './CodeEditor';

const LABEL = 'Response body';

function renderEditor(props: Partial<CodeEditorProps> = {}) {
  const base: CodeEditorProps = { value: 'one\ntwo', language: 'text', label: LABEL };
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <CodeEditor {...base} {...props} />
    </I18nextProvider>,
  );
}

async function ready(container: HTMLElement): Promise<void> {
  await waitFor(() => {
    expect(editorCount(container, '.cm-content')).toBe(1);
  });
}

function contentOf(container: HTMLElement): Element | undefined {
  return Array.from(container.querySelectorAll('div'))
    .map((element) => element.shadowRoot?.querySelector('.cm-content'))
    .find((element): element is Element => element !== null && element !== undefined);
}

function problem(overrides: Partial<EditorDiagnostic> = {}): EditorDiagnostic {
  return { line: 2, column: 1, length: 3, code: 'bad', message: 'Bad token', ...overrides };
}

afterEach(() => {
  cleanup();
});

describe('CodeEditor', () => {
  it('loads the engine lazily, shows the text and states how to leave the editor', async () => {
    const { container } = renderEditor();

    expect(screen.getByText('Loading editor')).toBeTruthy();
    await ready(container);

    expect(editorText(container)).toContain('two');
    expect(screen.getByRole('group', { name: LABEL })).toBeTruthy();
    expect(screen.getByText('Esc then Tab leaves the editor')).toBeTruthy();
  });

  it('labels the editable content with the visible label', async () => {
    const { container } = renderEditor();
    await ready(container);

    const textbox = contentOf(container);
    expect(textbox?.getAttribute('aria-label')).toBe(LABEL);
    expect(textbox?.getAttribute('aria-invalid')).toBe('false');
  });

  it('reports typed input through onChange but not an external value change', async () => {
    const onChange = vi.fn();
    const ref = createRef<CodeEditorHandle>();
    const { container, rerender } = renderEditor({ onChange, ref });
    await ready(container);

    ref.current?.insertAtCursor('X');
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange.mock.calls[0]?.[0]).toContain('X');

    rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <CodeEditor value="replaced" language="text" label={LABEL} onChange={onChange} />
      </I18nextProvider>,
    );
    await waitFor(() => {
      expect(editorText(container)).toContain('replaced');
    });
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it('applies an insert requested before the engine has loaded, then takes focus', async () => {
    const onChange = vi.fn();
    const ref = createRef<CodeEditorHandle>();
    const { container } = renderEditor({ onChange, ref, value: '{}' });

    expect(screen.getByText('Loading editor')).toBeTruthy();
    ref.current?.insertAtCursor('X');
    ref.current?.focus();
    expect(onChange).not.toHaveBeenCalled();

    await ready(container);
    await waitFor(() => {
      expect(onChange).toHaveBeenCalledTimes(1);
    });
    expect(onChange.mock.calls[0]?.[0]).toContain('X');
    expect(document.activeElement).not.toBe(document.body);
  });

  it('lists errors and warnings with Go buttons and underlines the range', async () => {
    const diagnostics = [
      problem(),
      problem({ line: 1, column: 2, length: 1, code: 'soft', message: 'Soft', severity: 'warning' }),
    ];
    const { container } = renderEditor({ diagnostics });
    await ready(container);

    expect(screen.getByText('Errors 1')).toBeTruthy();
    expect(screen.getByText('Warnings 1')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Go to line 2, column 1: Bad token' })).toBeTruthy();
    await waitFor(() => {
      expect(editorCount(container, '.cm-lintRange-error')).toBe(1);
      expect(editorCount(container, '.cm-lintRange-warning')).toBe(1);
    });
  });

  it('announces the diagnostics count once they settle, and the all-clear afterwards', async () => {
    const { container, rerender } = renderEditor({ diagnostics: [problem()] });
    await ready(container);

    expect(await screen.findByText('1 error in Response body.')).toBeTruthy();

    rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <CodeEditor value={'one\ntwo'} language="text" label={LABEL} diagnostics={[]} />
      </I18nextProvider>,
    );
    expect(await screen.findByText('No problems in Response body.')).toBeTruthy();
  });

  it('stays silent when there were never any diagnostics', async () => {
    const { container } = renderEditor();
    await ready(container);

    await new Promise((resolve) => setTimeout(resolve, 800));
    expect(screen.queryByText(/No problems/)).toBeNull();
  });

  it('shows the size against the limit and flags going over it', async () => {
    const { container } = renderEditor({ value: 'abcdefghij', maxBytes: 4 });
    await ready(container);

    expect(screen.getByText(/over the limit/)).toBeTruthy();
    expect(screen.getByRole('group', { name: LABEL }).getAttribute('aria-describedby')).toContain(
      screen.getByText(/over the limit/).id,
    );
  });

  it('is read-only without a tab hint and without trapping focus', async () => {
    const { container } = renderEditor({ readOnly: true });
    await ready(container);

    expect(screen.getByText('Read only')).toBeTruthy();
    expect(screen.queryByText('Esc then Tab leaves the editor')).toBeNull();
    const content = contentOf(container);
    expect(content?.getAttribute('contenteditable')).toBe('false');
    expect(content?.getAttribute('aria-readonly')).toBe('true');
  });

  it('marks the single-line variant as one line and omits the tab hint', async () => {
    const { container } = renderEditor({ singleLine: true, value: 'x', labelHidden: true });
    await ready(container);

    const content = contentOf(container);
    expect(content?.getAttribute('aria-multiline')).toBe('false');
    expect(screen.queryByText('Esc then Tab leaves the editor')).toBeNull();
  });
});
