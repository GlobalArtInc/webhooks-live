import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { editorCount } from '../../features/diff/testDom';
import type { EditorDiagnostic } from '../../features/editor/types';
import { SCRIPT_META_FIXTURE } from '../../features/rules/script/metaFixture';
import { createI18nInstance } from '../../i18n/setup';
import { ScriptEditor, type ScriptEditorProps } from './ScriptEditor';

const LABEL = 'Script';

function renderEditor(props: Partial<ScriptEditorProps> = {}) {
  const base: ScriptEditorProps = { value: 'let a = 1;', label: LABEL, languageMeta: SCRIPT_META_FIXTURE };
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <ScriptEditor {...base} {...props} />
    </I18nextProvider>,
  );
}

afterEach(() => {
  cleanup();
});

describe('ScriptEditor', () => {
  it('names the editor and shows the size and the limits of the response phase', async () => {
    const { container } = renderEditor({ phase: 'before' });

    expect(screen.getByRole('group', { name: LABEL })).toBeTruthy();
    expect(screen.getByText('10 B of 16.0 KiB')).toBeTruthy();
    expect(screen.getByText('Before the response:')).toBeTruthy();
    expect(screen.getByText(/runs stop after 100 ms or 100,000 operations\./)).toBeTruthy();
    await waitFor(() => {
      expect(editorCount(container, '.cm-content')).toBe(1);
    });
  });

  it('shows the looser limits after the response', () => {
    renderEditor({ phase: 'after' });

    expect(screen.getByText('After the response:')).toBeTruthy();
    expect(screen.getByText(/runs stop after 1 sec or 1,000,000 operations\./)).toBeTruthy();
  });

  it('opens the language reference in a new tab without leaking the opener', () => {
    renderEditor();

    const link = screen.getByRole('link', { name: /Language reference/ });
    expect(link.textContent).toContain('(opens in a new tab)');
    expect(link.getAttribute('href')).toBe(SCRIPT_META_FIXTURE.reference_url);
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('rel')).toBe('noopener noreferrer');
  });

  it('says that language help is loading and keeps the editor usable meanwhile', () => {
    renderEditor({ languageMeta: null });

    expect(screen.getByText('Loading script language help')).toBeTruthy();
    expect(screen.getByRole('group', { name: LABEL })).toBeTruthy();
    expect(screen.queryByText('Before the response:')).toBeNull();
    expect(screen.queryByRole('link')).toBeNull();
  });

  it('reports a language help failure and retries on request', () => {
    const onRetryLanguage = vi.fn();
    renderEditor({ languageMeta: null, languageState: 'failed', onRetryLanguage });

    expect(screen.getByText('Language help could not load. Completion and hover docs are off.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onRetryLanguage).toHaveBeenCalledTimes(1);
  });

  it('marks a pending check without announcing it, and reports a failed check with a retry', () => {
    const onRetryValidation = vi.fn();
    const { rerender } = renderEditor({ validationState: 'pending' });

    const pending = screen.getByText('Checking the script');
    expect(pending.getAttribute('aria-hidden')).toBe('true');

    rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <ScriptEditor
          value="let a = 1;"
          label={LABEL}
          languageMeta={SCRIPT_META_FIXTURE}
          validationState="failed"
          onRetryValidation={onRetryValidation}
        />
      </I18nextProvider>,
    );
    expect(screen.getByText('The script could not be checked. Problems may be missing.')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onRetryValidation).toHaveBeenCalledTimes(1);
  });

  it('lists server diagnostics outside the editor with a Go button and underlines them', async () => {
    const diagnostics: EditorDiagnostic[] = [
      { line: 1, column: 9, length: 1, code: 'script_invalid', message: 'Expecting an expression', severity: 'error' },
    ];
    const { container } = renderEditor({ diagnostics });

    expect(screen.getByText('Errors 1')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Go to line 1, column 9: Expecting an expression' })).toBeTruthy();
    await waitFor(() => {
      expect(editorCount(container, '.cm-lintRange-error')).toBe(1);
    });
  });

  it('is read-only for a recorded run: no tab hint, no limits, still shows the problem line', async () => {
    const diagnostics: EditorDiagnostic[] = [
      { line: 1, column: 5, length: 1, code: 'script_runtime', message: 'Division by zero', severity: 'error' },
    ];
    const { container } = renderEditor({ readOnly: true, diagnostics });

    expect(screen.getByText('Read only')).toBeTruthy();
    expect(screen.queryByText('Esc then Tab leaves the editor')).toBeNull();
    expect(screen.queryByText('Before the response:')).toBeNull();
    expect(screen.getByRole('button', { name: 'Go to line 1, column 5: Division by zero' })).toBeTruthy();
    await waitFor(() => {
      expect(editorCount(container, '.cm-content')).toBe(1);
    });
  });

  it('reserves the full-height rows and exposes the offset to the editor theme', () => {
    const { container } = renderEditor({ fullHeight: true, offset: '20rem' });

    const root = container.firstElementChild as HTMLElement;
    expect(root.dataset.fullHeight).toBe('true');
    expect(root.style.getPropertyValue('--script-editor-offset')).toBe('20rem');
    const surface = screen.getByRole('group', { name: LABEL });
    expect(surface.style.getPropertyValue('--editor-rows')).toBe('20');
  });
});
