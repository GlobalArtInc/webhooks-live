import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { http, HttpResponse } from 'msw';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Capture } from '../../api/types';
import { createI18nInstance } from '../../i18n/setup';
import { ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { mockServer, registerMockServer } from '../../test/msw/server';
import type { Bytes } from '../body/types';
import { CaptureDetail } from '../detail/CaptureDetail';
import { EmailDetail } from './EmailDetail';
import { readEmailView } from './parseEmail';
import {
  DOCUMENT_PART_INDEX,
  EMAIL_CAPTURE_ID,
  FAKE_IMAGE_BODY,
  BOMB_PART_INDEX,
  BOMB_PNG_BODY,
  FAKE_PNG_PART_INDEX,
  hostileEmailCapture,
  PNG_BODY,
  PNG_PART_INDEX,
  SAFE_FILE_NAME,
  TEXT_PART_BODY,
  withMeta,
} from './testFixtures';

registerMockServer();

const PARTS_ROOT = `/api/v1/endpoints/${ENDPOINT_ID}/captures/${EMAIL_CAPTURE_ID}/parts`;
const EMPTY_BYTES: Bytes = new Uint8Array(0);
const noBody = (): Promise<Bytes> => Promise.resolve(EMPTY_BYTES);

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(URL, 'createObjectURL');
  Reflect.deleteProperty(URL, 'revokeObjectURL');
  vi.restoreAllMocks();
});

function renderEmail(capture: Capture = hostileEmailCapture) {
  const view = readEmailView(capture);
  if (view === null) {
    throw new Error('the fixture is not an email capture');
  }
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <EmailDetail view={view} loadBody={noBody} />
    </I18nextProvider>,
  );
}

function detailElement(capture: Capture) {
  return (
    <I18nextProvider i18n={createI18nInstance()}>
      <CaptureDetail
        state="ready"
        capture={capture}
        loadBody={noBody}
        bodyMode="pretty"
        onBodyModeChange={vi.fn()}
        wrap={false}
        onWrapChange={vi.fn()}
        collapsedSections={[]}
        onCollapsedSectionsChange={vi.fn()}
        onResend={vi.fn()}
        onDownloadRaw={vi.fn()}
        onDownloadFormFile={vi.fn()}
        onCopied={vi.fn()}
      />
    </I18nextProvider>
  );
}

function sectionOf(name: string): HTMLElement {
  return screen.getByRole('region', { name });
}

function editorText(container: HTMLElement): string {
  for (const element of container.querySelectorAll('div')) {
    if (element.shadowRoot !== null) {
      return element.shadowRoot.querySelector('.cm-content')?.textContent ?? '';
    }
  }
  return '';
}

function serveParts(): string[] {
  const requested: string[] = [];
  mockServer.use(
    http.get(`${PARTS_ROOT}/:index`, ({ params, request }) => {
      const index = Number(params.index);
      requested.push(`${index}${new URL(request.url).search}`);
      if (index === 0) {
        return new HttpResponse(TEXT_PART_BODY, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
      }
      if (index === PNG_PART_INDEX) {
        return new HttpResponse(PNG_BODY, { headers: { 'Content-Type': 'application/octet-stream' } });
      }
      if (index === FAKE_PNG_PART_INDEX) {
        return new HttpResponse(FAKE_IMAGE_BODY, { headers: { 'Content-Type': 'application/octet-stream' } });
      }
      if (index === BOMB_PART_INDEX) {
        return new HttpResponse(BOMB_PNG_BODY, { headers: { 'Content-Type': 'application/octet-stream' } });
      }
      return new HttpResponse(null, { status: 404 });
    }),
  );
  return requested;
}

describe('email detail', () => {
  it('shows hostile subject, sender and headers as inert text with visible escapes', () => {
    const { container } = renderEmail();

    const summary = sectionOf('Summary');
    expect(summary.textContent).toContain('Reset \\u{202E}drowssap\\u{2069} <img src=x onerror=alert(1)> now');
    expect(summary.textContent).toContain('\\u{202E}evil');
    const headers = within(sectionOf('Headers')).getByRole('table', { name: 'Message headers' });
    expect(headers.textContent).toContain('X-\\u{202E}Name');
    expect(headers.textContent).toContain('from mx.shop.example (mx.shop.example [192.0.2.10]) by in.hooks.example');
    expect(container.querySelector('img, script, iframe, object, embed, style')).toBeNull();
    expect(container.innerHTML).not.toContain('\u202E');
  });

  it('keeps very long header values bounded', () => {
    renderEmail();

    const cells = within(sectionOf('Headers')).getAllByRole('cell');
    const longest = Math.max(...cells.map((cell) => cell.textContent.length));
    expect(longest).toBeLessThan(2100);
  });

  it('lists attachments with downloads through the parts route and sanitized names', () => {
    renderEmail();

    const attachments = sectionOf('Attachments');
    const documentLink = within(attachments).getByRole('link', { name: 'Download ../../etc/\\u{202E}gpj.exe' });
    expect(documentLink.getAttribute('href')).toBe(`${PARTS_ROOT}/${DOCUMENT_PART_INDEX}?download=1`);
    expect(documentLink.getAttribute('download')).toBe(SAFE_FILE_NAME);
    const unnamed = within(attachments).getByRole('link', { name: 'Download (unnamed part 6)' });
    expect(unnamed.getAttribute('download')).toBe('part-6');
    for (const link of within(attachments).getAllByRole('link')) {
      expect(link.getAttribute('href')).toMatch(
        /^\/api\/v1\/endpoints\/[^/]+\/captures\/[^/]+\/parts\/\d+\?download=1$/,
      );
    }
  });

  it('offers a preview only for small raster images', () => {
    renderEmail();

    const previews = within(sectionOf('Attachments')).getAllByRole('button', { name: /^Preview /u });
    expect(previews.map((button) => button.getAttribute('aria-label'))).toEqual([
      'Preview logo.png',
      'Preview photo.png',
      'Preview bomb.png',
    ]);
  });

  it('previews a real image and refuses bytes that are not the declared image type', async () => {
    serveParts();
    const blobs: Blob[] = [];
    const createObjectURL = (blob: Blob): string => {
      blobs.push(blob);
      return 'blob:preview';
    };
    Object.defineProperty(URL, 'createObjectURL', { value: createObjectURL, configurable: true });
    Object.defineProperty(URL, 'revokeObjectURL', { value: vi.fn(), configurable: true });
    renderEmail();
    const attachments = sectionOf('Attachments');

    fireEvent.click(within(attachments).getByRole('button', { name: 'Preview logo.png' }));
    await waitFor(() => {
      expect(within(attachments).getByRole('img', { name: /PNG/u })).toBeTruthy();
    });
    expect(blobs.map((blob) => blob.type)).toEqual(['image/png']);

    fireEvent.click(within(attachments).getByRole('button', { name: 'Preview photo.png' }));
    await waitFor(() => {
      expect(within(attachments).getByRole('alert').textContent).toContain('The preview could not be shown');
    });
    expect(within(attachments).queryByRole('img')).toBeNull();
  });

  it('refuses to preview an image that declares more than sixteen megapixels', async () => {
    serveParts();
    Object.defineProperty(URL, 'createObjectURL', { value: () => 'blob:preview', configurable: true });
    Object.defineProperty(URL, 'revokeObjectURL', { value: vi.fn(), configurable: true });
    renderEmail();
    const attachments = sectionOf('Attachments');

    fireEvent.click(within(attachments).getByRole('button', { name: 'Preview bomb.png' }));

    await waitFor(() => {
      expect(within(attachments).getByRole('alert').textContent).toContain('10000 by 10000 pixels');
    });
    expect(within(attachments).queryByRole('img')).toBeNull();
  });

  it('shows the plain-text part from the text view as escaped text', async () => {
    const requested = serveParts();
    renderEmail();

    const text = sectionOf('Text');
    await waitFor(() => {
      expect(editorText(text)).toContain('Hello \\u{202E}there');
    });
    expect(editorText(text)).toContain('Second line with <script>alert(1)</script>');
    expect(requested).toContain('0?view=text');
  });

  it('opens the raw source on demand and escapes its control characters', async () => {
    renderEmail();
    const raw = sectionOf('Raw');
    expect(within(raw).queryByRole('button', { name: 'Download .eml' })).toBeNull();

    fireEvent.click(within(raw).getByRole('button', { name: 'Show Raw' }));

    await waitFor(() => {
      expect(editorText(raw)).toContain('--wl\\u{202E}');
    });
    expect(within(raw).getByRole('button', { name: 'Download .eml' })).toBeTruthy();
  });

  it('shows the parse error and truncated parts notices', () => {
    renderEmail(withMeta({ parse_error: 'bad \u202Eboundary', parts_truncated: true }));

    expect(screen.getByText(/could not fully parse/u)).toBeTruthy();
    expect(screen.getByText(/more parts than the server keeps/u)).toBeTruthy();
    expect(document.body.textContent).toContain('Parser message: bad \\u{202E}boundary');
  });

  it('shows no attachments section and a placeholder when the message has no parts', () => {
    renderEmail(withMeta({ parts: [] }));

    expect(screen.queryByRole('region', { name: 'Attachments' })).toBeNull();
    expect(within(sectionOf('Text')).getByText('No plain-text part')).toBeTruthy();
  });

  it('is used by the capture detail for email captures only', () => {
    render(detailElement(hostileEmailCapture));

    expect(screen.getByRole('group', { name: 'Email capture' })).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'Body' })).toBeNull();
  });

  it('starts from a clean state when the capture detail moves to another email', async () => {
    serveParts();
    Object.defineProperty(URL, 'createObjectURL', { value: () => 'blob:preview', configurable: true });
    Object.defineProperty(URL, 'revokeObjectURL', { value: vi.fn(), configurable: true });
    const view = render(detailElement(hostileEmailCapture));
    fireEvent.click(within(sectionOf('Attachments')).getByRole('button', { name: 'Preview logo.png' }));
    await waitFor(() => {
      expect(within(sectionOf('Attachments')).getByRole('img', { name: /PNG/u })).toBeTruthy();
    });
    fireEvent.click(within(sectionOf('Raw')).getByRole('button', { name: 'Show Raw' }));

    view.rerender(detailElement({ ...hostileEmailCapture, id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5e02' }));

    expect(within(sectionOf('Attachments')).queryByRole('img')).toBeNull();
    expect(within(sectionOf('Attachments')).getByRole('button', { name: 'Preview logo.png' })).toBeTruthy();
    expect(within(sectionOf('Raw')).getByRole('button', { name: 'Show Raw' })).toBeTruthy();
  });
});
