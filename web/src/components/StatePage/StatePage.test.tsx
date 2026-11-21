import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { StatePage } from './StatePage';

function renderPage(ui: ReactElement) {
  return render(<I18nextProvider i18n={createI18nInstance()}>{ui}</I18nextProvider>);
}

describe('StatePage', () => {
  afterEach(() => {
    cleanup();
  });

  it('shows the gone variant with a link to create a new endpoint', () => {
    renderPage(<StatePage kind="gone" />);

    expect(screen.getByRole('heading', { level: 1, name: 'This endpoint is gone' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Create endpoint' }).getAttribute('href')).toBe('/');
  });

  it('shows the not-found variant with a link home', () => {
    renderPage(<StatePage kind="not-found" />);

    expect(screen.getByRole('heading', { level: 1, name: 'Page not found' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Go home' }).getAttribute('href')).toBe('/');
  });

  it('shows the signed-in identity and calls onSignOut from the not-shared variant', () => {
    const onSignOut = vi.fn();
    renderPage(<StatePage kind="not-shared" detail={{ identity: 'dev@example.com' }} onSignOut={onSignOut} />);

    expect(screen.getByRole('heading', { level: 1, name: 'This endpoint is not shared with you' })).toBeTruthy();
    expect(screen.getByText('Signed in as dev@example.com')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(onSignOut).toHaveBeenCalledOnce();
  });

  it('shows the status and calls onRetry from the error variant', () => {
    const onRetry = vi.fn();
    renderPage(<StatePage kind="error" detail={{ status: 500 }} onRetry={onRetry} />);

    expect(screen.getByRole('heading', { level: 1, name: 'This page could not be loaded' })).toBeTruthy();
    expect(screen.getByText('Server answered with status 500')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it('links the sign-in variant to the given sign-in href with the provider name', () => {
    renderPage(
      <StatePage kind="signin" detail={{ providerName: 'GlobalArt SSO', signInHref: '/auth/login?return_to=%2F' }} />,
    );

    expect(screen.getByRole('heading', { level: 1, name: 'Sign in to continue' })).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Sign in with GlobalArt SSO' }).getAttribute('href')).toBe(
      '/auth/login?return_to=%2F',
    );
  });

  it('falls back to a plain sign-in label when the provider name is unknown', () => {
    renderPage(<StatePage kind="signin" detail={{ providerName: null }} />);

    expect(screen.getByRole('link', { name: 'Sign in' }).getAttribute('href')).toBe('/auth/login');
  });
  it('renders the extra actions slot next to the state actions', () => {
    renderPage(<StatePage kind="gone" actions={<button type="button">Remove from my endpoints</button>} />);

    expect(screen.getByRole('button', { name: 'Remove from my endpoints' })).toBeTruthy();
  });
});
