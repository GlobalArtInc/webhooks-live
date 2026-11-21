import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { editorCount } from '../../diff/testDom';
import { createI18nInstance } from '../../../i18n/setup';
import { SCRIPT_META_FIXTURE, SCRIPT_RUNTIME_DIAGNOSTIC, fixtureValidate } from './fixtures';
import { ScriptStepEditor, type ScriptStepEditorProps } from './ScriptStepEditor';
import type { ScriptTransport } from './types';

function transportWith(overrides: Partial<ScriptTransport> = {}): ScriptTransport {
  return {
    id: `test-${Math.random()}`,
    loadMeta: () => Promise.resolve(SCRIPT_META_FIXTURE),
    validate: (source) => Promise.resolve(fixtureValidate(source)),
    ...overrides,
  };
}

function renderStep(props: Partial<ScriptStepEditorProps>) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const base: ScriptStepEditorProps = { value: 'let a = 1;', label: 'Script' };
  return render(
    <QueryClientProvider client={client}>
      <I18nextProvider i18n={createI18nInstance()}>
        <ScriptStepEditor {...base} {...props} />
      </I18nextProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  cleanup();
});

describe('ScriptStepEditor', () => {
  it('loads the language meta and shows the limits line', async () => {
    renderStep({ transport: transportWith() });

    expect(await screen.findByText(/runs stop after 100 ms/)).toBeTruthy();
    expect(screen.getByRole('link', { name: /Language reference/ })).toBeTruthy();
  });

  it('marks the line of a syntax error that the server reports, after the debounce', async () => {
    const validate = vi.fn((source: string) => Promise.resolve(fixtureValidate(source)));
    const { container } = renderStep({
      transport: transportWith({ validate }),
      value: 'let a = 1;\nlet b = a +',
    });

    const go = await screen.findByRole('button', {
      name: 'Go to line 2, column 11: Expecting an expression after the operator',
    });
    expect(go).toBeTruthy();
    expect(screen.getByText('Errors 1')).toBeTruthy();
    await waitFor(() => {
      expect(editorCount(container, '.cm-lintRange-error')).toBe(1);
    });
    expect(validate).toHaveBeenCalledTimes(1);
    expect(validate.mock.calls[0]?.[0]).toBe('let a = 1;\nlet b = a +');
  });

  it('shows a failed language load, then the limits after Retry succeeds', async () => {
    const loadMeta = vi
      .fn<ScriptTransport['loadMeta']>()
      .mockRejectedValueOnce(new Error('offline'))
      .mockRejectedValueOnce(new Error('still offline'))
      .mockResolvedValue(SCRIPT_META_FIXTURE);
    renderStep({ transport: transportWith({ loadMeta }) });

    fireEvent.click(await screen.findByRole('button', { name: 'Retry' }));

    expect(await screen.findByText(/runs stop after 100 ms/)).toBeTruthy();
    expect(loadMeta).toHaveBeenCalledTimes(3);
  });

  it('shows a failed check as a notice and not as a problem list', async () => {
    const validate = vi.fn<ScriptTransport['validate']>().mockRejectedValue(new Error('offline'));
    renderStep({ transport: transportWith({ validate }) });

    expect(await screen.findByText('The script could not be checked. Problems may be missing.')).toBeTruthy();
    expect(screen.queryByText(/^Errors/)).toBeNull();
  });

  it('never asks the server when read-only and shows the recorded run error instead', async () => {
    const validate = vi.fn<ScriptTransport['validate']>().mockResolvedValue([]);
    renderStep({
      transport: transportWith({ validate }),
      readOnly: true,
      value: 'let total = request.json.items.len();',
      extraDiagnostics: [SCRIPT_RUNTIME_DIAGNOSTIC],
    });

    expect(
      await screen.findByRole('button', { name: `Go to line 1, column 13: ${SCRIPT_RUNTIME_DIAGNOSTIC.message}` }),
    ).toBeTruthy();
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(validate).not.toHaveBeenCalled();
  });
});
