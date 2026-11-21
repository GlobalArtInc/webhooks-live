import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ReactElement } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../i18n/setup';
import { Announcer } from './Announcer/Announcer';
import { ConnectionIndicator } from './ConnectionIndicator/ConnectionIndicator';
import { FollowToggle } from './FollowToggle/FollowToggle';
import { NewPill } from './NewPill/NewPill';
import type { ConnectionState } from '../features/live/types';

const NOW = Date.parse('2026-11-21T10:10:00.000Z');

function renderWithI18n(element: ReactElement) {
  return render(<I18nextProvider i18n={createI18nInstance()}>{element}</I18nextProvider>);
}

describe('live components', () => {
  afterEach(cleanup);

  it('names every connection state with text and a status prefix for assistive technology', () => {
    const labels: readonly (readonly [ConnectionState, string])[] = [
      ['open', 'Live'],
      ['connecting', 'Connecting...'],
      ['reconnecting', 'Reconnecting...'],
      ['offline', 'Offline'],
      ['ended', 'Ended'],
    ];
    for (const [state, label] of labels) {
      const view = renderWithI18n(<ConnectionIndicator state={state} lastEventAt={null} now={NOW} />);

      expect(view.container.textContent).toBe(`Connection status: ${label}`);
      view.unmount();
    }
  });

  it('tells when the last event arrived in its tooltip', () => {
    const view = renderWithI18n(<ConnectionIndicator state="open" lastEventAt={NOW - 120_000} now={NOW} />);

    expect(view.container.firstElementChild?.getAttribute('title')).toBe('Last event 2 minutes ago');
  });

  it('keeps one polite status region that speaks the aggregated message', () => {
    const view = renderWithI18n(<Announcer announcement={null} muted={false} />);
    const region = screen.getByRole('status');
    expect(region.getAttribute('aria-live')).toBe('polite');
    expect(region.textContent).toBe('');

    view.rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <Announcer announcement={{ id: 1, count: 3, method: 'POST', path: '/stripe' }} muted={false} />
      </I18nextProvider>,
    );

    expect(screen.getByRole('status').textContent).toBe('3 new captures, latest POST /stripe');
  });

  it('keeps the region but says nothing when muted', () => {
    renderWithI18n(<Announcer announcement={{ id: 1, count: 1, method: 'GET', path: '/' }} muted />);

    expect(screen.getByRole('status').textContent).toBe('');
  });

  it('shows follow as pressed only while it is running and notes a pause', () => {
    const onToggle = vi.fn();
    const view = renderWithI18n(<FollowToggle enabled={false} paused={false} onToggle={onToggle} />);
    const button = screen.getByRole('button', { name: 'Follow' });
    expect(button.getAttribute('aria-pressed')).toBe('false');
    fireEvent.click(button);
    expect(onToggle).toHaveBeenCalledTimes(1);

    view.rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <FollowToggle enabled paused={false} onToggle={onToggle} />
      </I18nextProvider>,
    );
    expect(screen.getByRole('button', { name: 'Follow' }).getAttribute('aria-pressed')).toBe('true');

    view.rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <FollowToggle enabled paused onToggle={onToggle} />
      </I18nextProvider>,
    );
    expect(screen.getByRole('button', { name: 'Follow paused' }).getAttribute('aria-pressed')).toBe('false');
  });

  it('renders the pill only when something is new and reports the click', () => {
    const onActivate = vi.fn();
    const view = renderWithI18n(<NewPill count={0} onActivate={onActivate} />);
    expect(screen.queryByRole('button')).toBeNull();

    view.rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <NewPill count={1} onActivate={onActivate} />
      </I18nextProvider>,
    );
    fireEvent.click(screen.getByRole('button', { name: '1 new capture, scroll to the top' }));

    expect(onActivate).toHaveBeenCalledTimes(1);
  });
});
