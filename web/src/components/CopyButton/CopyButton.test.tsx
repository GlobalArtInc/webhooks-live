import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { CopyButton } from './CopyButton';

function renderButton(value: string) {
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <CopyButton value={value} label="Copy Host header" />
    </I18nextProvider>,
  );
}

function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
}

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(navigator, 'clipboard');
});

describe('CopyButton', () => {
  it('writes the value and shows the copied state under the row label', async () => {
    const writeText = vi.fn(() => Promise.resolve());
    stubClipboard(writeText);
    renderButton('Host: example.test');

    fireEvent.click(screen.getByRole('button', { name: 'Copy Host header' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Copy Host header' }).textContent).toBe('Copied');
    });
    expect(writeText).toHaveBeenCalledWith('Host: example.test');
  });

  it('shows the failed state when the clipboard rejects and no fallback copy succeeds', async () => {
    stubClipboard(() => Promise.reject(new Error('denied')));
    renderButton('value');

    fireEvent.click(screen.getByRole('button', { name: 'Copy Host header' }));

    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Copy Host header' }).textContent).toBe('Copy failed');
    });
  });
});
