import { act, cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import type { Endpoint } from '../../api/types';
import { renderAppAt } from '../../features/endpoints/renderApp';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { mockStore, seedEndpoint } from '../../test/msw/store';
import { captureSeries } from '../../features/endpoint/testFixtures';

registerMockServer();

afterEach(() => {
  cleanup();
});

const BASE_PATH = `/e/${ENDPOINT_ID}`;
const ENDPOINT_URL = `/api/v1/endpoints/${ENDPOINT_ID}`;
const NAV_LABEL = 'Endpoint configuration';
const REFUSED_MESSAGE = 'This address is refused. 10.0.0.5 is a private address';

function seed(): void {
  seedEndpoint(endpointFixture, []);
}

describe('endpoint workspace routes', () => {
  it('opens General for the bare settings path', async () => {
    seed();
    const { router } = renderAppAt(`${BASE_PATH}/settings`);

    expect(await screen.findByRole('heading', { name: 'General' })).toBeTruthy();
    await waitFor(() => expect(router.state.location.pathname).toBe(`${BASE_PATH}/settings/general`));
  });

  it('redirects the old forwarding path to the forwarding section', async () => {
    seed();
    const { router } = renderAppAt(`${BASE_PATH}/forwarding`);

    expect(await screen.findByRole('heading', { name: 'Forwarding' })).toBeTruthy();
    await waitFor(() => expect(router.state.location.pathname).toBe(`${BASE_PATH}/settings/forwarding`));
  });

  it('shows sections without an implementation as disabled navigation items', async () => {
    seed();
    renderAppAt(`${BASE_PATH}/settings/notifications`);

    const nav = await screen.findByRole('navigation', { name: NAV_LABEL });
    expect(within(nav).getByText('Notifications').getAttribute('aria-disabled')).toBe('true');
    expect(await screen.findByText('This section arrives in a later release.')).toBeTruthy();
  });

  it('saves the status code in one request and clears the unsaved state', async () => {
    seed();
    const bodies: unknown[] = [];
    mockServer.use(
      http.patch(ENDPOINT_URL, async ({ request }) => {
        bodies.push(await request.json());
        return jsonResponse({ ...endpointFixture, response: { ...endpointFixture.response, status: 418 } });
      }),
    );
    renderAppAt(`${BASE_PATH}/settings/response`);
    const status = await screen.findByLabelText('Status code');
    fireEvent.change(status, { target: { value: '418' } });
    const save = screen.getByRole('button', { name: 'Save' });
    fireEvent.click(save);

    await waitFor(() => expect(bodies).toHaveLength(1));
    expect(bodies[0]).toMatchObject({ response: { status: 418 } });
    await waitFor(() => expect(screen.getByRole('button', { name: 'Save' }).hasAttribute('disabled')).toBe(true));
  });

  it('asks before leaving a dirty section and keeps the edits when asked to', async () => {
    seed();
    const { router } = renderAppAt(`${BASE_PATH}/settings/general`);
    const title = await screen.findByLabelText('Title');
    fireEvent.change(title, { target: { value: 'renamed' } });

    const nav = screen.getByRole('navigation', { name: NAV_LABEL });
    fireEvent.click(within(nav).getByRole('link', { name: 'Response' }));
    const dialog = await screen.findByRole('dialog', { name: 'Discard unsaved changes?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Keep editing' }));

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(router.state.location.pathname).toBe(`${BASE_PATH}/settings/general`);
    expect((screen.getByLabelText('Title') as HTMLInputElement).value).toBe('renamed');
  });

  it('shows the conflict banner when the endpoint changes elsewhere, and Reload takes the remote title', async () => {
    seed();
    const { queryClient } = renderAppAt(`${BASE_PATH}/settings/general`);
    const title = await screen.findByLabelText('Title');
    fireEvent.change(title, { target: { value: 'mine' } });

    const current = queryClient.getQueryData<Endpoint>(['endpoint', ENDPOINT_ID]);
    act(() => {
      queryClient.setQueryData<Endpoint>(['endpoint', ENDPOINT_ID], {
        ...(current ?? endpointFixture),
        title: 'theirs',
      });
    });

    const banner = await screen.findByRole('alert');
    expect(within(banner).getByText('Changed elsewhere. Reload, or keep yours and overwrite.')).toBeTruthy();
    fireEvent.click(within(banner).getByRole('button', { name: 'Reload' }));

    await waitFor(() => expect((screen.getByLabelText('Title') as HTMLInputElement).value).toBe('theirs'));
    expect(screen.queryByText('Changed elsewhere. Reload, or keep yours and overwrite.')).toBeNull();
  });

  it('shows a refused forward target on its field', async () => {
    seed();
    renderAppAt(`${BASE_PATH}/settings/forwarding`);
    fireEvent.click(await screen.findByRole('button', { name: 'Add rule' }));
    fireEvent.change(screen.getByLabelText('Target URL'), { target: { value: 'http://10.0.0.5/hook' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save rule' }));

    const field = await screen.findByText(REFUSED_MESSAGE);
    expect(field.id).toMatch(/-error$/);
    expect(screen.getByLabelText('Target URL').getAttribute('aria-invalid')).toBe('true');
  });

  it('deletes the endpoint only after the typed confirmation matches its id', async () => {
    seed();
    renderAppAt(`${BASE_PATH}/settings/general`);
    fireEvent.click(await screen.findByRole('button', { name: 'Delete endpoint' }));

    const dialog = await screen.findByRole('dialog', { name: 'Delete this endpoint?' });
    const confirm = within(dialog).getByRole('button', { name: 'Delete endpoint' });
    expect(confirm.hasAttribute('disabled')).toBe(true);
    fireEvent.change(within(dialog).getByRole('textbox'), { target: { value: ENDPOINT_ID } });
    expect(confirm.hasAttribute('disabled')).toBe(false);
  });

  it('deletes all captures from General after confirmation', async () => {
    seedEndpoint({ ...endpointFixture, capture_count: 2 }, captureSeries(2));
    renderAppAt(`${BASE_PATH}/settings/general`);
    fireEvent.click(await screen.findByRole('button', { name: 'Delete all captures' }));

    const dialog = await screen.findByRole('dialog', { name: 'Delete all captures?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete all' }));

    await waitFor(() => expect(mockStore.captures.get(ENDPOINT_ID)?.length ?? 0).toBe(0));
  });

  it('does not save a status code outside the range and marks the field instead', async () => {
    seed();
    const bodies: unknown[] = [];
    mockServer.use(
      http.patch(ENDPOINT_URL, async ({ request }) => {
        bodies.push(await request.json());
        return jsonResponse(endpointFixture);
      }),
    );
    renderAppAt(`${BASE_PATH}/settings/response`);
    fireEvent.change(await screen.findByLabelText('Status code'), { target: { value: '99' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));

    await waitFor(() => expect(screen.getByLabelText('Status code').getAttribute('aria-invalid')).toBe('true'));
    expect(bodies).toHaveLength(0);
  });
});
