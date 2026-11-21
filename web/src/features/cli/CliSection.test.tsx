import { cleanup, fireEvent, screen } from '@testing-library/react';
import { http } from 'msw';
import { afterEach, describe, expect, it } from 'vitest';
import type { Instance } from '../../api/types';
import { ENDPOINT_ID, endpointFixture } from '../../test/fixtures/endpoint';
import { instanceFixture } from '../../test/fixtures/instance';
import { jsonResponse } from '../../test/msw/responses';
import { mockServer, registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';
import { renderAppAt } from '../endpoints/renderApp';
import { SIGNED_IN_USER } from '../tokens/testFixtures';

registerMockServer();

afterEach(() => {
  cleanup();
});

const SECTION_PATH = `/e/${ENDPOINT_ID}/settings/cli`;
const INSTANCE_PATH = '/api/v1/instance';

function serveInstance(overrides: Partial<Instance>): void {
  const instance: Instance = { ...instanceFixture, ...overrides };
  mockServer.use(http.get(INSTANCE_PATH, () => jsonResponse(instance)));
}

describe('local forwarding section', () => {
  it('builds the command from the capture URL with the default port and no token', async () => {
    seedEndpoint(endpointFixture, []);
    renderAppAt(SECTION_PATH);

    const command = await screen.findByRole('region', { name: 'Command' });
    expect(command.textContent).toBe(`webhooks-live-cli forward ${endpointFixture.url} --to http://localhost:3000`);
    expect(command.textContent).not.toContain('WEBHOOKS_LIVE_TOKEN');
  });

  it('shows an error and no command for a port outside 1 to 65535', async () => {
    seedEndpoint(endpointFixture, []);
    renderAppAt(SECTION_PATH);

    const port = await screen.findByLabelText('Local port');
    fireEvent.change(port, { target: { value: '65536' } });

    expect(port.getAttribute('aria-invalid')).toBe('true');
    expect(screen.getByText('Enter a port from 1 to 65535.')).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'Command' })).toBeNull();
  });

  it('links to the API tokens page when the instance has tokens', async () => {
    serveInstance({ auth_mode: 'sso', user: SIGNED_IN_USER });
    seedEndpoint(endpointFixture, []);
    renderAppAt(SECTION_PATH);

    const link = await screen.findByRole('link', { name: 'Create a token on the API tokens page' });
    expect(link.getAttribute('href')).toBe('/settings/tokens');
  });

  it('switches the install tab with a click and with the arrow keys', async () => {
    seedEndpoint(endpointFixture, []);
    renderAppAt(SECTION_PATH);

    fireEvent.click(await screen.findByRole('tab', { name: 'macOS Apple silicon' }));
    const archive = screen.getByRole('link', { name: 'Download the archive' });
    expect(archive.getAttribute('href')).toMatch(
      /\/cli-v0\.1\.0-dev\/webhooks-live-cli-0\.1\.0-dev-macos-arm64\.tar\.gz$/,
    );

    fireEvent.keyDown(screen.getByRole('tab', { name: 'macOS Apple silicon' }), { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: 'macOS Intel' }).getAttribute('aria-selected')).toBe('true');
  });
});
