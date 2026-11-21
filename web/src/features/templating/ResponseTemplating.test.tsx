import { act, cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { queryKeys } from '../../api/queries';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import {
  INVALID_TEMPLATE_BODY,
  templatedEndpoint,
  VALID_TEMPLATE_BODY,
  WARNING_TEMPLATE_BODY,
} from '../../test/fixtures/templating';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { addCapture, templatingHandlers, type TemplatingLog } from '../../test/msw/templating';
import { captureSeries } from '../endpoint/testFixtures';
import { renderAppAt } from '../endpoints/renderApp';

registerMockServer();

const RESPONSE_PATH = `/e/${ENDPOINT_ID}/settings/response`;
const ENDPOINT_URL = `/api/v1/endpoints/${ENDPOINT_ID}`;
const LATEST_ID = '00000000-0000-4000-8000-000000000002';
const FIRST_ID = '00000000-0000-4000-8000-000000000001';
const ERROR_COLUMN = INVALID_TEMPLATE_BODY.indexOf('isodate') + 1;
const SAVE_BLOCKED = 'Fix the template errors to save.';

let log: TemplatingLog;

beforeEach(() => {
  const mock = templatingHandlers();
  log = mock.log;
  mockServer.use(...mock.handlers);
});

afterEach(() => {
  cleanup();
});

async function previewBody(): Promise<HTMLElement> {
  return screen.findByText((_content, element) => element?.tagName === 'PRE' && element.textContent !== '');
}

describe('response templating', () => {
  it('renders a server preview for the latest capture and follows a new arrival', async () => {
    seedEndpoint(templatedEndpoint(VALID_TEMPLATE_BODY), captureSeries(2));
    const { queryClient } = renderAppAt(RESPONSE_PATH);

    const first = await previewBody();
    expect(first.textContent).toContain('"path": "/hook/2"');
    expect(first.textContent).toContain('"received": "evt_2"');
    expect(log.previews.at(-1)?.capture_id).toBe(LATEST_ID);
    expect(log.variableRequests).toContain(LATEST_ID);

    const arrived = addCapture(ENDPOINT_ID);
    await act(async () => {
      await queryClient.invalidateQueries({ queryKey: [...queryKeys.captures(ENDPOINT_ID), 'preview-sources'] });
    });

    await waitFor(() => expect(log.previews.at(-1)?.capture_id).toBe(arrived.id));
    await waitFor(async () => expect((await previewBody()).textContent).toContain('"path": "/hook/3"'));
  });

  it('pins the previewed capture into the URL, and unpins it again', async () => {
    seedEndpoint(templatedEndpoint(VALID_TEMPLATE_BODY), captureSeries(2));
    const { router } = renderAppAt(RESPONSE_PATH);
    await previewBody();

    fireEvent.click(screen.getByRole('button', { name: 'Pin' }));

    await waitFor(() => expect(router.state.location.href).toContain(`preview=${LATEST_ID}`));
    fireEvent.change(screen.getByLabelText('Preview with'), { target: { value: FIRST_ID } });
    await waitFor(() => expect(router.state.location.href).toContain(`preview=${FIRST_ID}`));
    await waitFor(() => expect(log.previews.at(-1)?.capture_id).toBe(FIRST_ID));
    await waitFor(async () => expect((await previewBody()).textContent).toContain('"path": "/hook/1"'));

    fireEvent.click(screen.getByRole('button', { name: 'Follow latest' }));
    await waitFor(() => expect(router.state.location.href).not.toContain('preview='));
  });

  it('lists an error from the server, keeps the last valid render and disables Save', async () => {
    seedEndpoint(templatedEndpoint(INVALID_TEMPLATE_BODY), captureSeries(2));
    renderAppAt(RESPONSE_PATH);

    const list = await screen.findByRole('group', { name: 'Template problems' });
    expect(within(list).getByText('Errors 1')).toBeTruthy();
    expect(within(list).getByText('Unknown filter "isodate"')).toBeTruthy();
    expect(within(list).getByText(`Body 1:${ERROR_COLUMN}`)).toBeTruthy();
    expect(screen.getByText('Nothing to show yet. Fix the errors in the editor.')).toBeTruthy();

    fireEvent.change(await screen.findByLabelText('Status code'), { target: { value: '201' } });

    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' }).hasAttribute('disabled')).toBe(true));
    expect(screen.getByText(SAVE_BLOCKED)).toBeTruthy();
    expect(
      screen.getByRole('button', { name: new RegExp(`^Go to Body, line 1, column ${ERROR_COLUMN}`) }),
    ).toBeTruthy();
  });

  it('shows an unknown variable as a warning and leaves Save enabled', async () => {
    seedEndpoint(templatedEndpoint(WARNING_TEMPLATE_BODY), captureSeries(2));
    renderAppAt(RESPONSE_PATH);

    const list = await screen.findByRole('group', { name: 'Template problems' });
    expect(within(list).getByText('Warnings 1')).toBeTruthy();
    expect(within(list).queryByText(/^Errors/)).toBeNull();
    await previewBody();

    fireEvent.change(await screen.findByLabelText('Status code'), { target: { value: '201' } });

    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' }).hasAttribute('disabled')).toBe(false));
    expect(screen.queryByText(SAVE_BLOCKED)).toBeNull();
  });

  it('saves the Use variables switch with the response', async () => {
    seedEndpoint(endpointFixture, captureSeries(1));
    const bodies: unknown[] = [];
    mockServer.use(
      http.patch(ENDPOINT_URL, async ({ request }) => {
        bodies.push(await request.json());
        return jsonResponse(templatedEndpoint(endpointFixture.response.body.data));
      }),
    );
    renderAppAt(RESPONSE_PATH);

    const toggle = await screen.findByRole('switch', { name: 'Use variables' });
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    expect(screen.getByText('Turn on Use variables to preview the response.')).toBeTruthy();
    expect(log.previews).toHaveLength(0);
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-checked')).toBe('true');
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toMatchObject({ response: { templated: true } });
  });

  it('sends a test request from an empty endpoint and pins the new capture', async () => {
    seedEndpoint(templatedEndpoint(VALID_TEMPLATE_BODY), []);
    const { router } = renderAppAt(RESPONSE_PATH);

    fireEvent.click(await screen.findByRole('button', { name: 'Send a test request to this endpoint' }));

    await waitFor(() => expect(log.sampleRequests).toBe(1));
    await waitFor(() => expect(router.state.location.href).toContain(`preview=${FIRST_ID}`));
    await waitFor(async () => expect((await previewBody()).textContent).toContain('"path": "/hook/1"'));
  });

  it('lets a viewer preview but not edit', async () => {
    seedEndpoint({ ...templatedEndpoint(VALID_TEMPLATE_BODY), my_role: 'viewer' }, captureSeries(2));
    renderAppAt(RESPONSE_PATH);

    await previewBody();
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull();
    expect(screen.getByRole('switch', { name: 'Use variables' }).hasAttribute('disabled')).toBe(true);
    expect(screen.getByText('You can read this response and preview it, but not change it.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Insert variable' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Render now' }).hasAttribute('disabled')).toBe(false);
  });
});
