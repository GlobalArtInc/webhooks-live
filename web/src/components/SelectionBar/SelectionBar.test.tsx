import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { SelectionBar, type SelectionBarProps } from './SelectionBar';

function renderWithI18n(element: ReactElement) {
  return render(<I18nextProvider i18n={createI18nInstance()}>{element}</I18nextProvider>);
}

function callbacks(): Pick<SelectionBarProps, 'onBegin' | 'onEnd' | 'onSelectAll' | 'onExport' | 'onDelete'> {
  return {
    onBegin: vi.fn(),
    onEnd: vi.fn(),
    onSelectAll: vi.fn(),
    onExport: vi.fn(),
    onDelete: vi.fn(),
  };
}

describe('SelectionBar', () => {
  afterEach(cleanup);

  it('offers only the select toggle and export until select mode is on', () => {
    const handlers = callbacks();
    renderWithI18n(<SelectionBar active={false} count={0} loadedCount={5} {...handlers} />);

    expect(screen.getByRole('button', { name: 'Select' }).getAttribute('aria-pressed')).toBe('false');
    expect(screen.queryByRole('button', { name: 'Delete selected' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Export' }));
    expect(handlers.onExport).toHaveBeenCalledTimes(1);
  });

  it('names the checked count and disables delete while nothing is checked', () => {
    const { unmount } = renderWithI18n(<SelectionBar active count={0} loadedCount={5} {...callbacks()} />);
    expect(screen.getByRole('status').textContent).toBe('0 selected');
    expect(screen.getByRole('button', { name: 'Delete selected' }).hasAttribute('disabled')).toBe(true);
    unmount();

    const handlers = callbacks();
    renderWithI18n(<SelectionBar active count={2} loadedCount={5} {...handlers} />);
    expect(screen.getByRole('status').textContent).toBe('2 selected');
    fireEvent.click(screen.getByRole('button', { name: 'Delete selected' }));
    expect(handlers.onDelete).toHaveBeenCalledTimes(1);
  });
});
