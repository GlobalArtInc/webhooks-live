import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { I18nProvider } from '../../i18n/provider';
import { AppShell } from './AppShell';

function Broken(): never {
  throw new Error('render failed');
}

function renderShell(children: ReactNode) {
  return render(
    <I18nProvider bundles={{}}>
      <AppShell resetKey="/">{children}</AppShell>
    </I18nProvider>,
  );
}

describe('AppShell', () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
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
});
