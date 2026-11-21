import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Capture } from '../../api/types';
import { curlBodyFileName, toCurl, toJson } from '../../features/export';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { createI18nInstance } from '../../i18n/setup';
import { CopyAsMenu, type CopyAsMenuProps } from './CopyAsMenu';

const LARGE_BODY_BYTES = 300 * 1024;

const largeCapture: Capture = {
  ...jsonCaptureFixture,
  body_size: LARGE_BODY_BYTES,
  body: { encoding: 'utf8', data: 'a'.repeat(LARGE_BODY_BYTES), truncated: false },
};

const truncatedCapture: Capture = {
  ...jsonCaptureFixture,
  body_size: 2 * 1024 * 1024,
  body: { encoding: 'utf8', data: 'a'.repeat(16), truncated: true },
};

const brokenHeaderCapture: Capture = {
  ...jsonCaptureFixture,
  headers: [...jsonCaptureFixture.headers, { name: 'X-Broken', value: 'a\rb' }],
};

const writeText = vi.fn(() => Promise.resolve());
let downloads: string[] = [];

function renderMenu(overrides: Partial<CopyAsMenuProps> = {}) {
  const props: CopyAsMenuProps = {
    capture: jsonCaptureFixture,
    onCopied: vi.fn(),
    ...overrides,
  };
  return {
    onCopied: props.onCopied,
    ...render(
      <I18nextProvider i18n={createI18nInstance()}>
        <CopyAsMenu {...props} />
      </I18nextProvider>,
    ),
  };
}

function openMenu(): void {
  fireEvent.keyDown(screen.getByRole('button', { name: 'Copy' }), { key: 'Enter' });
}

beforeAll(() => {
  URL.createObjectURL = vi.fn(() => 'blob:copy-as-test');
  URL.revokeObjectURL = vi.fn();
});

beforeEach(() => {
  writeText.mockClear();
  downloads = [];
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (this: HTMLAnchorElement) {
    downloads.push(this.download);
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  Reflect.deleteProperty(navigator, 'clipboard');
});

describe('CopyAsMenu', () => {
  it('copies the curl command produced by the export function', async () => {
    const { onCopied } = renderMenu();
    openMenu();

    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy as curl' }));

    await waitFor(() => {
      expect(onCopied).toHaveBeenCalledWith('curl');
    });
    expect(writeText).toHaveBeenCalledWith(toCurl(jsonCaptureFixture));
    expect(downloads).toEqual([]);
  });

  it('copies the JSON export produced by the export function', async () => {
    renderMenu();
    openMenu();

    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy as JSON' }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith(toJson(jsonCaptureFixture));
    });
  });

  it('copies the curl command and downloads the body file it refers to for a large body', async () => {
    renderMenu({ capture: largeCapture });
    openMenu();

    fireEvent.click(screen.getByRole('menuitem', { name: 'Copy as curl' }));

    await waitFor(() => {
      expect(downloads).toEqual([curlBodyFileName(largeCapture)]);
    });
    expect(writeText).toHaveBeenCalledWith(toCurl(largeCapture));
  });

  it('disables the item with the export error as its reason for a truncated body', () => {
    renderMenu({ capture: truncatedCapture });
    openMenu();

    const item = screen.getByRole('menuitem', { name: /Copy as curl/ });

    expect(item.getAttribute('aria-disabled')).toBe('true');
    expect(item.textContent).toContain('Unavailable: Body is truncated; fetch the full body before exporting');
  });

  it('warns about headers curl cannot send', () => {
    renderMenu({ capture: brokenHeaderCapture });

    expect(screen.getByText('1 header is not sent by curl: X-Broken')).toBeTruthy();
  });
});
