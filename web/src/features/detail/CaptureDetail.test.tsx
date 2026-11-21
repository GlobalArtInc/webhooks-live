import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Capture } from '../../api/types';
import {
  binaryCaptureFixture,
  formCaptureFixture,
  forwardedCaptureFixture,
  jsonCaptureFixture,
} from '../../test/fixtures/captures';
import { createI18nInstance } from '../../i18n/setup';
import type { Bytes } from '../body';
import { CaptureDetail, type CaptureDetailProps } from './CaptureDetail';

const EMPTY_BYTES: Bytes = new Uint8Array(0);

const multiValuedCapture: Capture = {
  ...jsonCaptureFixture,
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a09',
  query_string: 'tag=a&tag=b',
  query: [
    { name: 'tag', value: 'a' },
    { name: 'tag', value: 'b' },
  ],
  headers: [
    { name: 'Host', value: 'webhooks.example.test' },
    { name: 'Content-Type', value: 'application/json' },
    { name: 'X-Dup', value: 'first' },
    { name: 'X-Dup', value: 'second' },
    ...Array.from({ length: 10 }, (_, index) => ({ name: `X-Row-${index + 1}`, value: String(index) })),
  ],
};

const markupCapture: Capture = {
  ...jsonCaptureFixture,
  headers: [...jsonCaptureFixture.headers, { name: 'X-Markup', value: '<img src=x onerror="alert(1)">' }],
};

const truncatedCapture: Capture = {
  ...jsonCaptureFixture,
  body_size: 2 * 1024 * 1024,
  body: { encoding: 'utf8', data: 'a'.repeat(16), truncated: true },
};

function renderDetail(capture: Capture | null, overrides: Partial<CaptureDetailProps> = {}) {
  const props: CaptureDetailProps = {
    state: 'ready',
    capture,
    loadBody: () => Promise.resolve(EMPTY_BYTES),
    bodyMode: 'pretty',
    onBodyModeChange: vi.fn(),
    wrap: false,
    onWrapChange: vi.fn(),
    collapsedSections: [],
    onCollapsedSectionsChange: vi.fn(),
    onResend: vi.fn(),
    onDownloadRaw: vi.fn(),
    onDownloadFormFile: vi.fn(),
    onCopied: vi.fn(),
    ...overrides,
  };
  return {
    props,
    ...render(
      <I18nextProvider i18n={createI18nInstance()}>
        <CaptureDetail {...props} />
      </I18nextProvider>,
    ),
  };
}

function sectionNames(): string[] {
  return screen
    .queryAllByRole('region')
    .map((region) => region.getAttribute('aria-labelledby'))
    .map((headingId) => (headingId === null ? '' : (document.getElementById(headingId)?.textContent ?? '')))
    .filter((name) => name !== '');
}

const writeText = vi.fn(() => Promise.resolve());

beforeAll(() => {
  Object.defineProperty(Element.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});

beforeEach(() => {
  writeText.mockClear();
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
});

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(navigator, 'clipboard');
});

describe('CaptureDetail sections', () => {
  it('shows summary, query, body and headers for a JSON capture', () => {
    renderDetail(jsonCaptureFixture);

    expect(sectionNames()).toEqual(['Summary', 'Query', 'Body', 'Headers']);
  });

  it('shows the form section with its files for a multipart capture', () => {
    const onDownloadFormFile = vi.fn();
    renderDetail(formCaptureFixture, { onDownloadFormFile });

    expect(sectionNames()).toEqual(['Summary', 'Form', 'Body', 'Headers']);
    expect(screen.getByRole('cell', { name: 'invoice.pdf' })).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Download invoice' }));

    expect(onDownloadFormFile).toHaveBeenCalledWith(formCaptureFixture.form?.files[0]);
  });

  it('hides the query, form and forward sections for a binary capture', () => {
    renderDetail(binaryCaptureFixture);

    expect(sectionNames()).toEqual(['Summary', 'Body', 'Headers']);
  });

  it('shows the forward section with a status chip and a resend button for a forwarded capture', () => {
    const onResend = vi.fn();
    renderDetail(forwardedCaptureFixture, { onResend });

    expect(sectionNames()).toEqual(['Summary', 'Forward', 'Body', 'Headers']);
    expect(within(screen.getByRole('table', { name: 'Forward' })).getByText('200')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Resend' }));

    expect(onResend).toHaveBeenCalledOnce();
  });

  it('keeps repeated query rows and collapses headers to twelve rows until Show all', () => {
    renderDetail(multiValuedCapture);

    const query = screen.getByRole('table', { name: 'Query' });
    expect(within(query).getAllByRole('rowheader', { name: 'tag' })).toHaveLength(2);

    const headers = screen.getByRole('table', { name: 'Headers' });
    expect(within(headers).getAllByRole('row')).toHaveLength(13);

    fireEvent.click(screen.getByRole('button', { name: 'Show all 14' }));

    expect(within(headers).getAllByRole('row')).toHaveLength(15);
    expect(within(headers).getAllByRole('rowheader', { name: 'X-Dup' })).toHaveLength(2);
  });

  it('renders a header value containing markup as text', () => {
    const { container } = renderDetail(markupCapture);

    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByRole('cell', { name: '<img src=x onerror="alert(1)">' })).toBeTruthy();
  });

  it('copies a header row as "name: value" from its row button', async () => {
    renderDetail(jsonCaptureFixture);

    fireEvent.click(screen.getByRole('button', { name: 'Copy Host' }));

    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith('Host: webhooks.example.test');
    });
  });

  it('reports collapse toggles and hides the content of a collapsed section', () => {
    const { props } = renderDetail(jsonCaptureFixture, { collapsedSections: ['headers'] });

    expect(screen.queryByRole('table', { name: 'Headers' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Show Headers' }).getAttribute('aria-expanded')).toBe('false');

    fireEvent.click(screen.getByRole('button', { name: 'Show Headers' }));

    expect(props.onCollapsedSectionsChange).toHaveBeenCalledWith([]);
  });

  it('scrolls to a section and moves focus to its heading when the nav is used', () => {
    renderDetail(forwardedCaptureFixture);

    fireEvent.click(screen.getByRole('button', { name: 'Forward' }));

    expect(Element.prototype.scrollIntoView).toHaveBeenCalled();
    expect(document.activeElement?.textContent).toBe('Forward');
  });

  it('marks a large body as pending with a note in the body section', () => {
    renderDetail(truncatedCapture);

    expect(
      screen.getByText('Only the stored part of this body is here. The viewer loads the rest on request.'),
    ).toBeTruthy();
  });
});

describe('CaptureDetail states', () => {
  it('shows the loading status without sections', () => {
    renderDetail(null, { state: 'loading' });

    expect(screen.getByRole('status').textContent).toBe('Loading capture');
    expect(screen.queryAllByRole('region')).toHaveLength(0);
  });

  it('shows the deleted placeholder', () => {
    renderDetail(null, { state: 'deleted' });

    expect(screen.getByRole('status').textContent).toBe('This capture was deleted');
  });

  it('shows the error alert', () => {
    renderDetail(null, { state: 'error' });

    expect(screen.getByRole('alert').textContent).toBe('Could not load this capture');
  });
});
