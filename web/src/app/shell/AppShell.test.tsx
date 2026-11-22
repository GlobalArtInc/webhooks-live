import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { I18nProvider } from '../../i18n/provider';
import { PREFS_STORAGE_KEY } from '../../prefs/theme';
import { AppShell } from './AppShell';
import { useRailState } from './railState';

function Broken(): never {
  throw new Error('render failed');
}

function RailProbe() {
  const { collapsed, toggleCollapsed } = useRailState();
  return (
    <button type="button" onClick={toggleCollapsed}>
      {collapsed ? 'collapsed' : 'expanded'}
    </button>
  );
}

function renderShell(children: ReactNode) {
  return render(
    <I18nProvider bundles={{}}>
      <AppShell resetKey="/">{children}</AppShell>
    </I18nProvider>,
  );
}

function renderRail(collapseRail: boolean) {
  return render(
    <I18nProvider bundles={{}}>
      <AppShell resetKey="/" rail={<RailProbe />} collapseRail={collapseRail}>
        <p>Body</p>
      </AppShell>
    </I18nProvider>,
  );
}

function storedRail(): unknown {
  const raw = window.localStorage.getItem(PREFS_STORAGE_KEY);
  return raw === null ? undefined : (JSON.parse(raw) as Record<string, unknown>).rail;
}

describe('AppShell', () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it('exposes the banner, the main landmark, and a skip link to the main region', async () => {
    renderShell(<p>Body</p>);

    expect(await screen.findByRole('banner')).toBeTruthy();
    expect(screen.getByRole('main').getAttribute('id')).toBe('main');
    expect(screen.getByRole('link', { name: 'Skip to main content' }).getAttribute('href')).toBe('#main');
  });

  it('moves focus to the main region when the skip link is activated', async () => {
    renderShell(<p>Body</p>);

    fireEvent.click(await screen.findByRole('link', { name: 'Skip to main content' }));

    expect(document.activeElement).toBe(screen.getByRole('main'));
  });

  it('keeps the header and renders the error variant inside main when a child throws', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    renderShell(<Broken />);

    expect(await screen.findByRole('heading', { level: 1, name: 'This page could not be loaded' })).toBeTruthy();
    expect(screen.getByRole('banner')).toBeTruthy();
    expect(screen.getByRole('main').textContent).toContain('This page could not be loaded');
  });

  it('renders the stored collapsed rail on the first paint and keeps the choice after a reload', async () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, '{"rail":"collapsed"}');

    renderRail(false);

    expect(await screen.findByRole('button', { name: 'collapsed' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'collapsed' }));
    expect(storedRail()).toBe('expanded');

    cleanup();
    renderRail(false);
    expect(await screen.findByRole('button', { name: 'expanded' })).toBeTruthy();
  });

  it('persists a manual collapse on an ordinary route', async () => {
    renderRail(false);

    fireEvent.click(await screen.findByRole('button', { name: 'expanded' }));

    expect(storedRail()).toBe('collapsed');
  });

  it('collapses on an auto-collapse route without touching the stored choice, and a manual expand there is not stored', async () => {
    const { rerender } = renderRail(true);

    expect(await screen.findByRole('button', { name: 'collapsed' })).toBeTruthy();
    expect(storedRail()).toBeUndefined();
    fireEvent.click(screen.getByRole('button', { name: 'collapsed' }));
    expect(screen.getByRole('button', { name: 'expanded' })).toBeTruthy();
    expect(storedRail()).toBeUndefined();

    rerender(
      <I18nProvider bundles={{}}>
        <AppShell resetKey="/other" rail={<RailProbe />} collapseRail={false}>
          <p>Body</p>
        </AppShell>
      </I18nProvider>,
    );
    expect(screen.getByRole('button', { name: 'expanded' })).toBeTruthy();
  });

  it('returns to the stored collapsed choice after leaving an auto-collapse route', async () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, '{"rail":"collapsed"}');
    const { rerender } = renderRail(true);
    fireEvent.click(await screen.findByRole('button', { name: 'collapsed' }));

    rerender(
      <I18nProvider bundles={{}}>
        <AppShell resetKey="/other" rail={<RailProbe />} collapseRail={false}>
          <p>Body</p>
        </AppShell>
      </I18nProvider>,
    );

    expect(screen.getByRole('button', { name: 'collapsed' })).toBeTruthy();
    expect(storedRail()).toBe('collapsed');
  });
});
