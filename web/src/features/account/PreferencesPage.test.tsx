import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { COMPACT_CLASS } from '../../prefs/display';
import { PREFS_STORAGE_KEY } from '../../prefs/theme';
import { I18nProvider } from '../../i18n/provider';
import { PreferencesPage } from './PreferencesPage';

function renderPage() {
  return render(
    <I18nProvider bundles={{}}>
      <PreferencesPage />
    </I18nProvider>,
  );
}

function storedFields(): Record<string, unknown> {
  return JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? '{}') as Record<string, unknown>;
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  document.documentElement.classList.remove(COMPACT_CLASS);
  document.documentElement.removeAttribute('data-theme');
});

describe('PreferencesPage', () => {
  it('applies a density choice at once and stores it', async () => {
    renderPage();

    fireEvent.click(await screen.findByRole('radio', { name: 'Compact' }));

    expect(document.documentElement.classList.contains(COMPACT_CLASS)).toBe(true);
    expect(storedFields()).toMatchObject({ density: 'compact' });
  });

  it('reset all returns every preference to its default and reports it', async () => {
    window.localStorage.setItem(
      PREFS_STORAGE_KEY,
      JSON.stringify({
        theme: 'dark',
        density: 'compact',
        timeDisplay: 'utc',
        singleKeyShortcuts: false,
        follow: true,
      }),
    );
    document.documentElement.classList.add(COMPACT_CLASS);
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Reset all preferences' }));

    expect(document.documentElement.classList.contains(COMPACT_CLASS)).toBe(false);
    expect(storedFields()).toMatchObject({
      theme: 'system',
      density: 'comfortable',
      timeDisplay: 'relative',
      singleKeyShortcuts: true,
      follow: false,
      announce: true,
    });
    expect(await screen.findByText('Preferences were reset to their defaults.')).toBeTruthy();
    expect(screen.getByRole('switch', { name: 'Single-key shortcuts' }).getAttribute('aria-checked')).toBe('true');
  });
});
