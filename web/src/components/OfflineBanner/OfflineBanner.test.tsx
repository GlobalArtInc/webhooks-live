import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { OfflineBanner } from './OfflineBanner';

const OFFLINE_MESSAGE = 'You are offline. Shown data may be out of date.';

function setOnline(value: boolean): void {
  Object.defineProperty(window.navigator, 'onLine', { configurable: true, get: () => value });
}

function renderBanner() {
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <OfflineBanner />
    </I18nextProvider>,
  );
}

describe('OfflineBanner', () => {
  afterEach(() => {
    cleanup();
    Reflect.deleteProperty(window.navigator, 'onLine');
  });

  it('stays hidden while the browser is online', () => {
    setOnline(true);
    renderBanner();

    expect(screen.getByRole('status').textContent).toBe('');
  });

  it('announces the offline state and dismisses itself when back online', () => {
    setOnline(false);
    renderBanner();

    expect(screen.getByRole('status').textContent).toBe(OFFLINE_MESSAGE);

    setOnline(true);
    fireEvent(window, new Event('online'));

    expect(screen.getByRole('status').textContent).toBe('');
  });
});
