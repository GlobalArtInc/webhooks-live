import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { SendRequest } from '../../api/types';
import { I18nProvider } from '../../i18n/provider';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { sentRequestFixture, SENT_TARGET } from '../../test/fixtures/composer';
import { errorResponse, jsonResponse } from '../../test/msw/responses';
import { sendHandlers } from '../../test/msw/composer';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { ComposerWorkspace } from './ComposerWorkspace';
import { emptyDraft, seedFromCapture, type ComposerSeed } from './draft';

registerMockServer();

const URL_LABEL = 'Target URL';
const SEND_LABEL = 'Send';
const BLANK: ComposerSeed = { draft: emptyDraft(), banner: null, sourceCaptureId: null };

afterEach(() => {
  cleanup();
});

function renderWorkspace(seed: ComposerSeed = BLANK) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <I18nProvider bundles={{}}>
        <ComposerWorkspace endpointId={null} seed={seed} />
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('composer workspace', () => {
  it('replays a captured request with the same body and shows the server response', async () => {
    const requests: SendRequest[] = [];
    mockServer.use(...sendHandlers({ requests }));
    renderWorkspace(seedFromCapture(jsonCaptureFixture));

    expect(await screen.findByText('Replay of POST /404/orders?x=1')).toBeTruthy();
    fireEvent.change(screen.getByLabelText(URL_LABEL), {
      target: { value: `https://erp.example.test${jsonCaptureFixture.path}?x=1` },
    });
    fireEvent.click(screen.getByRole('button', { name: SEND_LABEL }));

    await waitFor(() => {
      expect(requests).toHaveLength(1);
    });
    expect(requests[0]).toMatchObject({
      method: 'POST',
      body: { encoding: 'utf8', data: '{"order":42}' },
      source_capture_id: jsonCaptureFixture.id,
    });
    expect(await screen.findByText('200')).toBeTruthy();
  });

  it('puts a refused target on the URL field with the server reason', async () => {
    mockServer.use(
      ...sendHandlers({
        reply: () =>
          errorResponse(422, 'validation_failed', 'url address 10.0.0.5 is private', [
            { field: 'url', code: 'target_refused', message: 'address 10.0.0.5 is private' },
          ]),
      }),
    );
    renderWorkspace();

    fireEvent.change(await screen.findByLabelText(URL_LABEL), { target: { value: 'http://10.0.0.5/hook' } });
    fireEvent.click(screen.getByRole('button', { name: SEND_LABEL }));

    expect(await screen.findByText('This address is refused. address 10.0.0.5 is private')).toBeTruthy();
    expect(screen.getByLabelText(URL_LABEL).getAttribute('aria-invalid')).toBe('true');
  });

  it('explains a spent quota with the limit and the retry delay', async () => {
    mockServer.use(
      ...sendHandlers({
        reply: () =>
          jsonResponse(
            {
              error: {
                code: 'quota_exceeded',
                message: 'composer quota exhausted',
                limit: { name: 'composer_user_hour', window: 'hour', max: 30, resets_at: '2026-11-21T11:00:00.000Z' },
              },
            },
            429,
            { 'Retry-After': '120' },
          ),
      }),
    );
    renderWorkspace();

    fireEvent.change(await screen.findByLabelText(URL_LABEL), { target: { value: 'https://example.test/hook' } });
    fireEvent.click(screen.getByRole('button', { name: SEND_LABEL }));

    expect(
      await screen.findByText('You have used your 30 requests for this hour. Try again in 120 seconds.'),
    ).toBeTruthy();
  });

  it('shows a redacted history header as such and does not send it until the value is entered again', async () => {
    const requests: SendRequest[] = [];
    mockServer.use(...sendHandlers({ requests, history: [sentRequestFixture] }));
    renderWorkspace();

    fireEvent.click(await screen.findByRole('button', { name: `Replay POST ${SENT_TARGET}` }));

    expect(await screen.findByDisplayValue('[redacted]')).toBeTruthy();
    expect(screen.getByText('Redacted in history. Enter the real value before sending.')).toBeTruthy();
    expect((screen.getByLabelText(URL_LABEL) as HTMLInputElement).value).toBe(SENT_TARGET);
    fireEvent.click(screen.getByRole('button', { name: SEND_LABEL }));
    expect(requests).toEqual([]);
  });
});
