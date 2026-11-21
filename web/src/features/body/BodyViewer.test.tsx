import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { BodyViewer } from './BodyViewer';
import {
  BINARY_BODY,
  encode,
  generateLargeBody,
  HTML_BODY,
  INVALID_JSON_BODY,
  JSON_BODY,
  MIB,
  PNG_1X1,
  TEXT_BODY,
  XML_BODY,
} from './fixtures';
import type { BodyFormatter, BodyViewerProps, Bytes, FormatResult } from './types';

const BINARY_SHA256 = 'c44d343ffc7415a2914c78986df18d31b6780909f30c5b03d2e58cdcfe022803';

function createFormatter(result: FormatResult): BodyFormatter {
  return { format: vi.fn(() => Promise.resolve(result)), dispose: vi.fn() };
}

function propsFor(body: Bytes, contentType: string, overrides: Partial<BodyViewerProps> = {}): BodyViewerProps {
  return {
    bodyId: 'cap_1',
    body: { type: 'bytes', bytes: body },
    contentType,
    size: body.length,
    mode: 'pretty',
    wrap: false,
    search: '',
    onModeChange: vi.fn(),
    onWrapChange: vi.fn(),
    onSearchChange: vi.fn(),
    onDownloadRaw: vi.fn(),
    ...overrides,
  };
}

function editorText(container: HTMLElement): string {
  return container.querySelector('.cm-content')?.textContent ?? '';
}

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(URL, 'createObjectURL');
  Reflect.deleteProperty(URL, 'revokeObjectURL');
  vi.restoreAllMocks();
});

describe('BodyViewer', () => {
  it('shows a JSON body pretty-printed with the full set of controls', async () => {
    const { container } = render(<BodyViewer {...propsFor(encode(JSON_BODY), 'application/json')} />);

    await waitFor(() => {
      expect(editorText(container)).toContain('"object"');
    });
    expect(editorText(container)).toContain('"livemode": false,');
    expect(screen.getByRole('button', { name: 'Pretty' }).getAttribute('aria-pressed')).toBe('true');
    expect(screen.getByRole('button', { name: 'Raw' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Wrap' })).toBeTruthy();
    expect(screen.getByRole('searchbox', { name: 'Search in body' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Download raw' })).toBeTruthy();
  });

  it('shows invalid JSON raw with the error position', async () => {
    const { container } = render(<BodyViewer {...propsFor(encode(INVALID_JSON_BODY), 'application/json')} />);

    expect(screen.getByText('Not valid JSON at 4:13')).toBeTruthy();
    await waitFor(() => {
      expect(editorText(container)).toContain('"status": ,');
    });
  });

  it('shows an XML body with the pretty and raw toggle', async () => {
    const { container } = render(<BodyViewer {...propsFor(encode(XML_BODY), 'application/xml')} />);

    await waitFor(() => {
      expect(editorText(container)).toContain('<name>Ada Lovelace</name>');
    });
    expect(screen.getByRole('button', { name: 'Pretty' })).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Raw' })).toBeTruthy();
  });

  it('shows a text body raw with a wrap toggle and no pretty toggle', async () => {
    const onWrapChange = vi.fn();
    const { container } = render(<BodyViewer {...propsFor(encode(TEXT_BODY), 'text/plain', { onWrapChange })} />);

    await waitFor(() => {
      expect(editorText(container)).toContain('deploy finished');
    });
    expect(screen.queryByRole('button', { name: 'Pretty' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Wrap' }));
    expect(onWrapChange).toHaveBeenCalledWith(true);
  });

  it('highlights in-body search matches and reports search input changes', async () => {
    const onSearchChange = vi.fn();
    const { container } = render(
      <BodyViewer {...propsFor(encode(TEXT_BODY), 'text/plain', { search: 'status', onSearchChange })} />,
    );

    await waitFor(() => {
      expect(container.querySelector('.cm-wl-match')?.textContent).toBe('status');
    });
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search in body' }), { target: { value: 'version' } });
    expect(onSearchChange).toHaveBeenCalledWith('version');
  });

  it('renders an HTML body as text and creates no script or img element from it', async () => {
    const { container } = render(<BodyViewer {...propsFor(encode(HTML_BODY), 'text/html')} />);

    await waitFor(() => {
      expect(editorText(container)).toContain('<script>document.title');
    });
    expect(editorText(container)).toContain('onerror="alert(1)"');
    expect(container.querySelector('script')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('iframe')).toBeNull();
    expect(container.querySelector('a')).toBeNull();
  });

  it('shows type, size, SHA-256, a hex view and a download control for a binary body', async () => {
    const onDownloadRaw = vi.fn();
    render(<BodyViewer {...propsFor(BINARY_BODY, 'application/octet-stream', { onDownloadRaw })} />);

    expect(screen.getByText('application/octet-stream, binary data')).toBeTruthy();
    expect(screen.getByText('20 B')).toBeTruthy();
    expect(screen.getByLabelText('Hex dump').textContent).toContain('00000000  00 01 02 03 ff fe fd 41');
    await waitFor(() => {
      expect(screen.getByText(BINARY_SHA256)).toBeTruthy();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Download raw' }));
    expect(onDownloadRaw).toHaveBeenCalledTimes(1);
  });

  it('previews an image through a blob typed from the magic bytes and revokes the URL on unmount', () => {
    const create = vi.fn((blob: Blob) => `blob:preview-${blob.type}`);
    const revoke = vi.fn();
    Object.assign(URL, { createObjectURL: create, revokeObjectURL: revoke });

    const { container, unmount } = render(<BodyViewer {...propsFor(PNG_1X1, 'text/html')} />);

    const image = container.querySelector('img');
    expect(image?.getAttribute('src')).toBe('blob:preview-image/png');
    expect(container.querySelector('.cm-content')).toBeNull();
    unmount();
    expect(revoke).toHaveBeenCalledWith('blob:preview-image/png');
  });

  it('does not pretty-print a 5 MiB body until it is requested, then does it in the formatter', async () => {
    const body = generateLargeBody(5 * MIB);
    const formatter = createFormatter({ ok: true, text: 'FORMATTED_BY_WORKER' });
    const props = propsFor(body, 'application/json', { formatter });
    const { container } = render(<BodyViewer {...props} />);

    expect(screen.getByRole('button', { name: 'Load full body (5.0 MiB)' })).toBeTruthy();
    await waitFor(() => {
      expect(container.querySelector('.cm-content')).not.toBeNull();
    });
    expect(formatter.format).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Load full body (5.0 MiB)' }));
    expect(formatter.format).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'Pretty' }));
    expect(formatter.format).toHaveBeenCalledTimes(1);
    expect(formatter.format).toHaveBeenCalledWith({ kind: 'json', bytes: body });
    await waitFor(() => {
      expect(editorText(container)).toContain('FORMATTED_BY_WORKER');
    });
  });

  it('fetches a large body through the loader: a 256 KiB prefix on open, the rest on request', async () => {
    const full = generateLargeBody(2 * MIB);
    const loadBody = vi.fn((limit: number | null) => Promise.resolve(limit === null ? full : full.slice(0, limit)));
    const props = propsFor(full, 'application/json', { body: undefined, loadBody });
    render(<BodyViewer {...props} />);

    await screen.findByRole('button', { name: /Load full body/ });
    expect(loadBody).toHaveBeenCalledTimes(1);
    expect(loadBody).toHaveBeenLastCalledWith(256 * 1024);

    fireEvent.click(screen.getByRole('button', { name: /Load full body/ }));
    await waitFor(() => {
      expect(loadBody).toHaveBeenLastCalledWith(null);
    });
  });
});
