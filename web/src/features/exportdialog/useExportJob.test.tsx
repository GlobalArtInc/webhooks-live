import { act, renderHook, waitFor } from '@testing-library/react';
import { HttpResponse, http } from 'msw';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { endpointFixture } from '../../test/fixtures/endpoint';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { useExportJob } from './useExportJob';

registerMockServer();

const ENDPOINT_ID = endpointFixture.id;
const EXPORT_PATH = `/api/v1/endpoints/${ENDPOINT_ID}/export`;

describe('useExportJob', () => {
  beforeEach(() => {
    seedEndpoint(endpointFixture);
    Object.assign(URL, { createObjectURL: vi.fn(() => 'blob:export'), revokeObjectURL: vi.fn() });
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('downloads the selected captures under the file name the server sent', async () => {
    mockServer.use(
      http.post(EXPORT_PATH, () =>
        HttpResponse.text('[]', {
          headers: {
            'Content-Type': 'application/json',
            'Content-Disposition': 'attachment; filename="webhooks-live-export.json"',
          },
        }),
      ),
    );
    const { result } = renderHook(() => useExportJob());

    act(() => {
      result.current.start({
        kind: 'ids',
        endpointId: ENDPOINT_ID,
        input: { format: 'json', capture_ids: ['capture-a'] },
      });
    });

    await waitFor(() => {
      expect(result.current.phase.kind).toBe('ready');
    });
    expect(result.current.phase).toMatchObject({ kind: 'ready', fileName: 'webhooks-live-export.json' });
    expect(HTMLAnchorElement.prototype.click).toHaveBeenCalledTimes(1);
  });

  it('ignores an answer that arrives after the export was cancelled', async () => {
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    let answered = false;
    mockServer.use(
      http.post(EXPORT_PATH, async () => {
        await gate;
        answered = true;
        return HttpResponse.text('[]', { headers: { 'Content-Type': 'application/json' } });
      }),
    );
    const { result } = renderHook(() => useExportJob());

    act(() => {
      result.current.start({
        kind: 'ids',
        endpointId: ENDPOINT_ID,
        input: { format: 'zip', capture_ids: ['capture-a'] },
      });
    });
    expect(result.current.phase.kind).toBe('running');

    act(() => {
      result.current.cancel();
    });
    expect(result.current.phase.kind).toBe('cancelled');

    await act(async () => {
      release();
      await waitFor(() => {
        expect(answered).toBe(true);
      });
      await new Promise<void>((resolve) => {
        setTimeout(resolve, 0);
      });
    });
    expect(result.current.phase.kind).toBe('cancelled');
    expect(HTMLAnchorElement.prototype.click).not.toHaveBeenCalled();
  });
});
