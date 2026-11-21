import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { BodyViewer } from './BodyViewer';
import { BINARY_BODY, encode, HTML_BODY, INVALID_JSON_BODY, JSON_BODY, PNG_1X1, TEXT_BODY, XML_BODY } from './fixtures';
import type { BodyViewerProps, Bytes } from './types';

const BINARY_SHA256 = 'c44d343ffc7415a2914c78986df18d31b6780909f30c5b03d2e58cdcfe022803';

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

function editorRoot(container: HTMLElement): ShadowRoot | null {
  for (const element of container.querySelectorAll('div')) {
    if (element.shadowRoot !== null) {
      return element.shadowRoot;
    }
  }
  return null;
}

function inEditor(container: HTMLElement, selector: string): Element | null {
  return editorRoot(container)?.querySelector(selector) ?? null;
}

function editorText(container: HTMLElement): string {
  return inEditor(container, '.cm-content')?.textContent ?? '';
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
      expect(inEditor(container, '.cm-wl-match')?.textContent).toBe('status');
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
    for (const selector of ['script', 'img', 'iframe', 'a']) {
      expect(inEditor(container, selector)).toBeNull();
    }
  });

  it('mounts the editor in a shadow root and adds no style element to document.head', async () => {
    const stylesBefore = document.head.querySelectorAll('style').length;
    const { container } = render(<BodyViewer {...propsFor(encode(JSON_BODY), 'application/json')} />);

    await waitFor(() => {
      expect(inEditor(container, '.cm-content')).not.toBeNull();
    });
    expect(container.querySelector('.cm-content')).toBeNull();
    expect(document.head.querySelectorAll('style')).toHaveLength(stylesBefore);
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
    expect(editorRoot(container)).toBeNull();
    unmount();
    expect(revoke).toHaveBeenCalledWith('blob:preview-image/png');
  });
});
