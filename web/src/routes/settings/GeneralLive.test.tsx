import { act, cleanup, fireEvent, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderAppAt } from '../../features/endpoints/renderApp';
import { FakeEventSource } from '../../features/live/fakeEventSource';
import { resetLiveStores } from '../../features/live/store';
import { endpointFixture, endpointViewOf, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { registerMockServer } from '../../test/msw/server';
import { seedEndpoint } from '../../test/msw/store';

registerMockServer();

const streams: FakeEventSource[] = [];

class RecordedEventSource extends FakeEventSource {
  constructor(url: string) {
    super(url);
    streams.push(this);
  }
}

beforeEach(() => {
  streams.length = 0;
  vi.stubGlobal('EventSource', RecordedEventSource);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  resetLiveStores();
});

describe('General settings and the live stream', () => {
  it('keeps the form after the server announces the saved endpoint without its tags and role', async () => {
    seedEndpoint({ ...endpointFixture, tags: ['billing'], my_role: null }, []);
    renderAppAt(`/e/${ENDPOINT_ID}/settings/general`);
    const title = await screen.findByLabelText('Title');
    await waitFor(() => expect(streams.length).toBeGreaterThan(0));
    const stream = streams.at(-1);
    if (stream === undefined) {
      throw new Error('the workspace did not open a stream');
    }
    act(() => {
      stream.open();
    });

    fireEvent.change(title, { target: { value: 'renamed' } });
    act(() => {
      stream.send('endpoint_updated', endpointViewOf({ ...endpointFixture, title: 'renamed' }), '1');
    });

    await waitFor(() => expect((screen.getByLabelText('Title') as HTMLInputElement).value).toBe('renamed'));
    expect(screen.queryByText('This page could not be loaded')).toBeNull();
    expect(screen.getByRole('heading', { name: 'General' })).toBeTruthy();
  });
});
