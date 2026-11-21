import { cleanup, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { renderAppAt } from '../../features/endpoints/renderApp';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { ENDPOINT_ID, endpointFixture } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

afterEach(() => {
  cleanup();
});

describe('composer route', () => {
  it('opens the replay of the capture named in the search parameters', async () => {
    seedEndpoint(endpointFixture, [jsonCaptureFixture]);
    renderAppAt(`/send?endpoint=${ENDPOINT_ID}&capture=${jsonCaptureFixture.id}`);

    expect(await screen.findByText('Replay of POST /404/orders?x=1')).toBeTruthy();
    expect((screen.getByLabelText('Target URL') as HTMLInputElement).value).toBe('/404/orders?x=1');
  });

  it('opens a blank composer and says so when the capture cannot be found', async () => {
    seedEndpoint(endpointFixture, []);
    renderAppAt(`/send?endpoint=${ENDPOINT_ID}&capture=${jsonCaptureFixture.id}`);

    expect(
      await screen.findByText('The captured request could not be loaded. Compose a new request instead.'),
    ).toBeTruthy();
    expect((screen.getByLabelText('Target URL') as HTMLInputElement).value).toBe('');
  });
});
