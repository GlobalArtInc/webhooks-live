import { cleanup, render } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { resolveImplicitId } from '../endpoint/implicitSelection';
import { createI18nInstance } from '../../i18n/setup';
import { summaryFor } from './liveFixtures';
import { countAbove, isAtTop } from './scrollAnchor';
import { badgeLabel, restoreFavicon, showFaviconBadge, tabTitle } from './tabBadge';
import { useTabBadge } from './useTabBadge';

const BASE_TITLE = 'webhooks.live';
const ORIGINAL_HREF = '/favicon.svg';
const DATA_URL = 'data:image/png;base64,AAAA';

interface DrawLog {
  texts: string[];
}

function stubCanvas(): DrawLog {
  const log: DrawLog = { texts: [] };
  const context = {
    beginPath: vi.fn(),
    roundRect: vi.fn(),
    fill: vi.fn(),
    stroke: vi.fn(),
    moveTo: vi.fn(),
    lineTo: vi.fn(),
    arc: vi.fn(),
    fillText: (text: string) => log.texts.push(text),
  };
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(context as unknown as CanvasRenderingContext2D);
  vi.spyOn(HTMLCanvasElement.prototype, 'toDataURL').mockReturnValue(DATA_URL);
  return log;
}

function setTokens(): void {
  const style = document.documentElement.style;
  style.setProperty('--color-text', '#111111');
  style.setProperty('--color-surface', '#ffffff');
  style.setProperty('--color-accent', '#d99a2b');
  style.setProperty('--color-text-on-accent', '#000000');
}

function iconLink(): HTMLLinkElement | null {
  return document.head.querySelector<HTMLLinkElement>('link[rel~="icon"]');
}

interface BadgeProps {
  count: number;
}

function Badge({ count }: BadgeProps) {
  useTabBadge(count);
  return null;
}

function renderBadge(count: number) {
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <Badge count={count} />
    </I18nextProvider>,
  );
}

describe('tab badge', () => {
  beforeEach(() => {
    document.head.innerHTML = `<link rel="icon" type="image/svg+xml" href="${ORIGINAL_HREF}">`;
    document.title = BASE_TITLE;
    setTokens();
    stubCanvas();
  });

  afterEach(() => {
    cleanup();
    restoreFavicon();
    vi.restoreAllMocks();
    document.documentElement.removeAttribute('style');
    document.head.innerHTML = '';
  });

  it('writes the unread count in front of the title and restores it at zero', () => {
    const view = renderBadge(3);
    expect(document.title).toBe(`(3) ${BASE_TITLE}`);

    view.rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <Badge count={0} />
      </I18nextProvider>,
    );

    expect(document.title).toBe(BASE_TITLE);
  });

  it('restores the title when the screen goes away', () => {
    const view = renderBadge(2);
    view.unmount();

    expect(document.title).toBe(BASE_TITLE);
  });

  it('formats the title string and the badge label', () => {
    expect(tabTitle(BASE_TITLE, 0, (count, title) => `(${count}) ${title}`)).toBe(BASE_TITLE);
    expect(tabTitle(BASE_TITLE, 12, (count, title) => `(${count}) ${title}`)).toBe(`(12) ${BASE_TITLE}`);
    expect([1, 9, 10, 250].map(badgeLabel)).toEqual(['1', '9', '9+', '9+']);
  });

  it('swaps the favicon for a drawn badge and puts the original back', () => {
    const log = stubCanvas();

    expect(showFaviconBadge(12)).toBe(true);
    expect(iconLink()?.getAttribute('href')).toBe(DATA_URL);
    expect(iconLink()?.getAttribute('type')).toBe('image/png');
    expect(log.texts).toEqual(['9+']);

    restoreFavicon();

    expect(iconLink()?.getAttribute('href')).toBe(ORIGINAL_HREF);
    expect(iconLink()?.getAttribute('type')).toBe('image/svg+xml');
  });

  it('drives the favicon from the hook and clears it at zero', () => {
    stubCanvas();
    const view = renderBadge(4);
    expect(iconLink()?.getAttribute('href')).toBe(DATA_URL);

    view.rerender(
      <I18nextProvider i18n={createI18nInstance()}>
        <Badge count={0} />
      </I18nextProvider>,
    );

    expect(iconLink()?.getAttribute('href')).toBe(ORIGINAL_HREF);
  });

  it('reports failure and leaves the favicon alone when no canvas is available', () => {
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);

    expect(showFaviconBadge(2)).toBe(false);
    expect(iconLink()?.getAttribute('href')).toBe(ORIGINAL_HREF);
  });

  it('creates a favicon link when the page has none and removes it again', () => {
    document.head.innerHTML = '';
    stubCanvas();

    showFaviconBadge(1);
    expect(iconLink()?.getAttribute('href')).toBe(DATA_URL);
    restoreFavicon();

    expect(iconLink()).toBeNull();
  });
});

describe('scroll anchoring helpers', () => {
  it('treats a scroll offset of a couple of pixels as the top', () => {
    expect(isAtTop(0)).toBe(true);
    expect(isAtTop(2)).toBe(true);
    expect(isAtTop(3)).toBe(false);
  });

  it('counts the rows that arrived above the row the reader left the top at', () => {
    const items = [summaryFor(7), summaryFor(6), summaryFor(5), summaryFor(4)];

    expect(countAbove(items, summaryFor(5).id)).toBe(2);
    expect(countAbove(items, summaryFor(7).id)).toBe(0);
    expect(countAbove(items, summaryFor(1).id)).toBe(0);
    expect(countAbove(items, null)).toBe(0);
  });
});

describe('implicit selection', () => {
  const items = [summaryFor(3), summaryFor(2), summaryFor(1)];

  it('keeps the pinned capture while follow is off, even when newer ones arrive', () => {
    expect(resolveImplicitId(items, summaryFor(2).id, false)).toBe(summaryFor(2).id);
  });

  it('tracks the newest capture while following and pins the newest at first sight', () => {
    expect(resolveImplicitId(items, summaryFor(2).id, true)).toBe(summaryFor(3).id);
    expect(resolveImplicitId(items, null, false)).toBe(summaryFor(3).id);
  });

  it('falls back to the newest when the pinned capture is gone or the list is empty', () => {
    expect(resolveImplicitId(items, 'deleted', false)).toBe(summaryFor(3).id);
    expect(resolveImplicitId([], 'deleted', false)).toBeNull();
  });
});
