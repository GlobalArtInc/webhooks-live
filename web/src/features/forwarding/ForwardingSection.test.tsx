import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import type { ForwardRequest, ForwardRule } from '../../api/types';
import { captureSeries } from '../endpoint/testFixtures';
import { renderAppAt } from '../endpoints/renderApp';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

afterEach(() => {
  cleanup();
});

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/forwarding`;
const RULE: ForwardRule = {
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a31',
  enabled: true,
  target_url: 'https://hooks.example.com/in',
  method: null,
  content_type: null,
  header_allowlist: [],
  append_path: false,
};

describe('forwarding section', () => {
  it('tests a rule against the latest capture and shows the forward result', async () => {
    seedEndpoint({ ...endpointFixture, forward_rules: [RULE] }, captureSeries(2));
    const bodies: ForwardRequest[] = [];
    mockServer.use(
      http.post(`/api/v1/endpoints/${ENDPOINT_ID}/captures/:captureId/forward`, async ({ request }) => {
        bodies.push((await request.json()) as ForwardRequest);
        return jsonResponse(
          {
            id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a41',
            kind: 'rule',
            rule_id: RULE.id,
            target_url: RULE.target_url,
            method: 'POST',
            state: 'pending',
            attempts: [],
            created_at: '2026-11-21T10:20:00.000Z',
            finished_at: null,
          },
          202,
        );
      }),
    );
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Test with latest capture' }));

    const table = await screen.findByRole('table', { name: 'Forward' });
    expect(within(table).getByText(RULE.target_url)).toBeTruthy();
    await waitFor(() => expect(bodies).toEqual([{ rule_id: RULE.id }]));
  });

  it('shows the no-capture notice when the endpoint has no captures yet', async () => {
    seedEndpoint({ ...endpointFixture, forward_rules: [RULE] }, []);
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Test with latest capture' }));

    expect(await screen.findByText('No captures yet. Send a request to the capture URL first.')).toBeTruthy();
  });

  it('switches a rule off and then deletes it', async () => {
    seedEndpoint({ ...endpointFixture, forward_rules: [RULE] }, []);
    renderAppAt(SECTION_PATH);
    const switchName = `Enabled for ${RULE.target_url}`;

    const toggle = await screen.findByRole('switch', { name: switchName });
    expect(toggle.getAttribute('aria-checked')).toBe('true');
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: switchName }).getAttribute('aria-checked')).toBe('false'),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Delete rule' }));
    expect(await screen.findByText('No forward rules yet. Captures are only stored.')).toBeTruthy();
  });
});
