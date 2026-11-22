import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { I18nProvider } from '../../i18n/provider';
import { CaptureUrl } from './CaptureUrl';

const URL_TEXT = `https://webhooks.live/abc/${'segment/'.repeat(300)}`;

function renderUrl(url: string) {
  return render(
    <I18nProvider bundles={{}}>
      <CaptureUrl method="GET" url={url} />
    </I18nProvider>,
  );
}

function fakeOverflow(clientHeight: number, scrollHeight: number): void {
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => clientHeight });
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, get: () => scrollHeight });
}

describe('capture url', () => {
  beforeEach(() => {
    fakeOverflow(80, 400);
  });

  afterEach(() => {
    cleanup();
    Reflect.deleteProperty(HTMLElement.prototype, 'clientHeight');
    Reflect.deleteProperty(HTMLElement.prototype, 'scrollHeight');
  });

  it('offers to expand a url that does not fit and switches back', async () => {
    renderUrl(URL_TEXT);
    const toggle = await screen.findByRole('button', { name: 'Show full URL' });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.getAttribute('aria-controls')).toBe(document.querySelector('code')?.id);

    fireEvent.click(toggle);
    const less = await screen.findByRole('button', { name: 'Show less' });
    expect(less.getAttribute('aria-expanded')).toBe('true');
    expect(document.querySelector('code')?.getAttribute('data-expanded')).toBe('true');

    fireEvent.click(less);
    expect(await screen.findByRole('button', { name: 'Show full URL' })).toBeTruthy();
  });

  it('shows no toggle for a url that fits and always keeps the copy button', async () => {
    fakeOverflow(80, 80);
    renderUrl('https://webhooks.live/abc/hook');

    expect(await screen.findByRole('button', { name: 'Copy URL' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Show full URL' })).toBeNull();
  });
});
