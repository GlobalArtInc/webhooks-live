import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactElement } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { PREFS_STORAGE_KEY } from '../../prefs/theme';
import { useShortcutHandlers, useShortcutScope } from './hooks';
import { ShortcutProvider } from './ShortcutProvider';

interface ProbeProps {
  next: () => void;
  close?: () => void;
}

function Probe({ next, close }: ProbeProps) {
  useShortcutScope('endpoint', true);
  useShortcutHandlers({ next, close });
  return (
    <div>
      <input aria-label="Filter" />
      <div className="cm-editor">
        <button type="button">Code</button>
      </div>
      <div
        onKeyDown={(event) => {
          event.preventDefault();
        }}
      >
        <button type="button">Handled</button>
      </div>
    </div>
  );
}

function ScopelessProbe({ next }: ProbeProps) {
  useShortcutHandlers({ next });
  return null;
}

function renderWith(ui: ReactElement) {
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <ShortcutProvider>{ui}</ShortcutProvider>
    </I18nextProvider>,
  );
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe('ShortcutProvider', () => {
  it('runs the bound handler and prevents the default action of the key', () => {
    const next = vi.fn();
    renderWith(<Probe next={next} />);

    const notPrevented = fireEvent.keyDown(document.body, { key: 'j' });

    expect(next).toHaveBeenCalledTimes(1);
    expect(notPrevented).toBe(false);
  });

  it('ignores keys typed into a text field', () => {
    const next = vi.fn();
    renderWith(<Probe next={next} />);

    fireEvent.keyDown(screen.getByRole('textbox', { name: 'Filter' }), { key: 'j' });

    expect(next).not.toHaveBeenCalled();
  });

  it('ignores keys pressed inside the code viewer', () => {
    const next = vi.fn();
    renderWith(<Probe next={next} />);

    fireEvent.keyDown(screen.getByRole('button', { name: 'Code' }), { key: 'j' });

    expect(next).not.toHaveBeenCalled();
  });

  it('ignores keys another component has already handled', () => {
    const next = vi.fn();
    renderWith(<Probe next={next} />);

    fireEvent.keyDown(screen.getByRole('button', { name: 'Handled' }), { key: 'j' });

    expect(next).not.toHaveBeenCalled();
  });

  it('ignores shortcuts pressed with Ctrl or Meta', () => {
    const next = vi.fn();
    renderWith(<Probe next={next} />);

    fireEvent.keyDown(document.body, { key: 'j', ctrlKey: true });
    fireEvent.keyDown(document.body, { key: 'j', metaKey: true });

    expect(next).not.toHaveBeenCalled();
  });

  it('does nothing for a scope that no component has enabled', () => {
    const next = vi.fn();
    renderWith(<ScopelessProbe next={next} />);

    fireEvent.keyDown(document.body, { key: 'j' });

    expect(next).not.toHaveBeenCalled();
  });

  it('puts the initial focus of the help dialog on its title, not on the first control', async () => {
    renderWith(<Probe next={vi.fn()} close={vi.fn()} />);

    fireEvent.keyDown(document.body, { key: '?' });

    const title = screen.getByRole('heading', { name: 'Keyboard shortcuts' });
    await waitFor(() => {
      expect(document.activeElement).toBe(title);
    });
    expect(document.activeElement).not.toBe(screen.getByRole('switch', { name: 'Single-key shortcuts' }));
  });

  it('opens the help dialog with ?, blocks shortcuts while it is open, and turns single keys off from it', () => {
    const next = vi.fn();
    const close = vi.fn();
    renderWith(<Probe next={next} close={close} />);

    fireEvent.keyDown(document.body, { key: '?' });
    const dialog = screen.getByRole('dialog', { name: 'Keyboard shortcuts' });
    expect(screen.getByText('Next capture (older)')).toBeTruthy();

    fireEvent.keyDown(dialog, { key: 'j' });
    expect(next).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('switch', { name: 'Single-key shortcuts' }));
    expect(screen.getByRole('switch', { name: 'Single-key shortcuts' }).getAttribute('aria-checked')).toBe('false');
    expect(JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? '')).toEqual({ singleKeyShortcuts: false });

    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();

    fireEvent.keyDown(document.body, { key: 'j' });
    expect(next).not.toHaveBeenCalled();

    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(close).toHaveBeenCalledTimes(1);
  });
});
