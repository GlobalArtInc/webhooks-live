import { cleanup, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { jsonResponse, readJsonBody } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { schemaProperties } from '../../test/openapiShapes';
import { renderAppAt } from '../endpoints/renderApp';

registerMockServer();

afterEach(() => {
  cleanup();
});

const CLONE_URL = `/api/v1/endpoints/${ENDPOINT_ID}/clone`;

async function openDialog() {
  seedEndpoint(endpointFixture, []);
  const { router } = renderAppAt(`/e/${ENDPOINT_ID}/settings/general`);
  fireEvent.click(await screen.findByRole('button', { name: 'Clone endpoint' }));
  const dialog = await screen.findByRole('dialog', { name: 'Clone endpoint' });
  return { dialog, router };
}

describe('clone dialog', () => {
  it('sends only the fields of the real CloneInput and opens the copy', async () => {
    const bodies: Record<string, unknown>[] = [];
    mockServer.use(
      http.post(CLONE_URL, async ({ request }) => {
        const body = await readJsonBody<Record<string, unknown>>(request);
        bodies.push(body ?? {});
        return jsonResponse(
          { ...endpointFixture, id: 'cloned0000000000000000aa', secrets_missing: [], skipped: [] },
          201,
        );
      }),
    );
    const { dialog, router } = await openDialog();
    fireEvent.click(within(dialog).getByRole('checkbox', { name: 'Schedules (copied paused)' }));
    fireEvent.click(within(dialog).getByRole('button', { name: 'Clone' }));

    await waitFor(() => expect(bodies).toHaveLength(1));
    const [body] = bodies;
    expect(Object.keys(body ?? {}).sort()).toEqual(schemaProperties('CloneInput').sort());
    expect(body?.include).toEqual(['response', 'forward_rules', 'rules', 'channels']);
    await waitFor(() => expect(router.state.location.pathname).toBe('/e/cloned0000000000000000aa/settings/general'));
    expect(screen.queryByText('The endpoint could not be cloned.')).toBeNull();
  });

  it('names the secrets that the copy lacks before opening it', async () => {
    mockServer.use(
      http.post(CLONE_URL, () =>
        jsonResponse(
          { ...endpointFixture, id: 'cloned0000000000000000aa', secrets_missing: ['KEY'], skipped: [] },
          201,
        ),
      ),
    );
    const { dialog } = await openDialog();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Clone' }));

    expect(await screen.findByText('Secrets were not copied')).toBeTruthy();
    expect(screen.getByText('KEY')).toBeTruthy();
  });
});
