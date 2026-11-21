import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { BodyViewer } from './BodyViewer';
import { generateLargeBody, MIB } from './fixtures';
import { LENGTH_ATTRIBUTE } from './lightEditor.testing';
import type { BodyFormatter, BodyViewerProps, Bytes, FormatTask, FormatResult } from './types';

vi.mock('./editor', () => import('./lightEditor.testing'));

const HEAVY_TEST_TIMEOUT_MS = 30_000;
const PREFIX_BYTES = 256 * 1024;
const FORMATTED_TEXT = 'FORMATTED_BY_WORKER';
const FIVE_MIB_BODY = generateLargeBody(5 * MIB);
const TWO_MIB_BODY = generateLargeBody(2 * MIB);

function createFormatter(result: FormatResult): BodyFormatter {
  return { format: vi.fn(() => Promise.resolve(result)), dispose: vi.fn() };
}

function propsFor(body: Bytes, overrides: Partial<BodyViewerProps> = {}): BodyViewerProps {
  return {
    bodyId: 'cap_1',
    body: { type: 'bytes', bytes: body },
    contentType: 'application/json',
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

function shownLength(container: HTMLElement): number | null {
  const attribute = container.querySelector('.cm-content')?.getAttribute(LENGTH_ATTRIBUTE);
  return attribute === null || attribute === undefined ? null : Number(attribute);
}

function shownText(container: HTMLElement): string {
  return container.querySelector('.cm-content')?.textContent ?? '';
}

function sameBytes(left: Bytes, right: Bytes): boolean {
  if (left.length !== right.length) {
    return false;
  }
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) {
      return false;
    }
  }
  return true;
}

function onlyRequest(formatter: BodyFormatter): FormatTask {
  const calls = vi.mocked(formatter.format).mock.calls;
  expect(calls).toHaveLength(1);
  const request = calls[0]?.[0];
  if (request === undefined) {
    throw new Error('the formatter was not called');
  }
  return request;
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe('BodyViewer with a large body', () => {
  it(
    'does not pretty-print a 5 MiB body until it is requested, then does it in the formatter',
    { timeout: HEAVY_TEST_TIMEOUT_MS },
    async () => {
      const formatter = createFormatter({ ok: true, text: FORMATTED_TEXT });
      const { container } = render(<BodyViewer {...propsFor(FIVE_MIB_BODY, { formatter })} />);

      const loadFull = screen.getByRole('button', { name: 'Load full body (5.0 MiB)' });
      await waitFor(() => {
        expect(shownLength(container)).toBe(PREFIX_BYTES);
      });
      expect(formatter.format).not.toHaveBeenCalled();

      fireEvent.click(loadFull);
      await waitFor(() => {
        expect(shownLength(container)).toBe(FIVE_MIB_BODY.length);
      });
      expect(formatter.format).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole('button', { name: 'Pretty' }));
      const request = onlyRequest(formatter);
      expect(request.kind).toBe('json');
      expect(sameBytes(request.bytes, FIVE_MIB_BODY)).toBe(true);
      await waitFor(() => {
        expect(shownText(container)).toContain(FORMATTED_TEXT);
      });
    },
  );

  it(
    'fetches a large body through the loader: a 256 KiB prefix on open, the rest on request',
    { timeout: HEAVY_TEST_TIMEOUT_MS },
    async () => {
      const loadBody = vi.fn((limit: number | null) =>
        limit === null ? new Promise<Bytes>(() => undefined) : Promise.resolve(TWO_MIB_BODY.slice(0, limit)),
      );
      render(<BodyViewer {...propsFor(TWO_MIB_BODY, { body: undefined, loadBody })} />);

      const loadFull = await screen.findByRole('button', { name: /Load full body/ });
      expect(loadBody).toHaveBeenCalledTimes(1);
      expect(loadBody).toHaveBeenLastCalledWith(PREFIX_BYTES);

      fireEvent.click(loadFull);
      await waitFor(() => {
        expect(loadBody).toHaveBeenLastCalledWith(null);
      });
    },
  );
});
