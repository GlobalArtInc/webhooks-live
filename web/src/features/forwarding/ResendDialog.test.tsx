import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { captureIdFor, captureSeries } from '../endpoint/testFixtures';
import { I18nProvider } from '../../i18n/provider';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { ResendDialog } from './ResendDialog';

registerMockServer();

afterEach(() => {
  cleanup();
});

const TARGET = 'https://hooks.example.com/once';

describe('resend dialog', () => {
  it('sends a one-off resend to the typed target and shows its result', async () => {
    seedEndpoint(endpointFixture, captureSeries(1));
    const bodies: unknown[] = [];
    mockServer.use(
      http.post(`/api/v1/endpoints/${ENDPOINT_ID}/captures/:captureId/forward`, async ({ request }) => {
        bodies.push(await request.json());
        return jsonResponse(
          {
            id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a51',
            kind: 'manual',
            rule_id: null,
            target_url: TARGET,
            method: 'POST',
            state: 'pending',
            attempts: [],
            created_at: '2026-11-21T10:30:00.000Z',
            finished_at: null,
          },
          202,
        );
      }),
    );
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={client}>
        <I18nProvider bundles={{}}>
          <ResendDialog endpointId={ENDPOINT_ID} captureId={captureIdFor(1)} open onOpenChange={vi.fn()} />
        </I18nProvider>
      </QueryClientProvider>,
    );

    fireEvent.change(await screen.findByLabelText('Target URL'), { target: { value: TARGET } });
    fireEvent.click(screen.getByRole('button', { name: 'Send' }));

    await waitFor(() => expect(bodies).toEqual([{ target_url: TARGET, method: null }]));
    const table = await screen.findByRole('table', { name: 'Forward' });
    expect(within(table).getByText(TARGET)).toBeTruthy();
  });
});
