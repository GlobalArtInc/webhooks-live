import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { CodeEditorHandle } from '../../features/editor/types';
import { createI18nInstance } from '../../i18n/setup';
import { CodeEditor } from './CodeEditor';

vi.mock('../../features/editor/engine', () => {
  throw new Error('chunk failed');
});

afterEach(() => {
  cleanup();
});

describe('CodeEditor when the editor chunk cannot load', () => {
  it('falls back to a labelled text field that still edits and inserts', async () => {
    const onChange = vi.fn();
    const ref = createRef<CodeEditorHandle>();
    render(
      <I18nextProvider i18n={createI18nInstance()}>
        <CodeEditor ref={ref} value="abc" language="text" label="Body" onChange={onChange} />
      </I18nextProvider>,
    );

    const field = await screen.findByRole('textbox', { name: 'Body' });
    expect(screen.getByRole('alert').textContent).toContain('plain text field');

    fireEvent.change(field, { target: { value: 'abcd' } });
    expect(onChange).toHaveBeenLastCalledWith('abcd');

    (field as HTMLTextAreaElement).setSelectionRange(1, 1);
    ref.current?.insertAtCursor('Z');
    expect(onChange).toHaveBeenLastCalledWith('aZbc');
  });
});
