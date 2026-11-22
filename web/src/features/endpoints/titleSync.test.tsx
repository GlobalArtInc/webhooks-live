import { QueryClient } from '@tanstack/react-query';
import { afterEach, describe, expect, it } from 'vitest';
import { queryKeys } from '../../api/queries';
import { createAppQueryClient } from '../../app/providers';
import { endpointFixture } from '../../test/fixtures/endpoint';
import { ENDPOINTS_STORAGE_KEY, parseEndpoints, serializeEndpoints } from './storage';
import { rememberEndpoint } from './store';

const ID = endpointFixture.id;

function stored() {
  return parseEndpoints(window.localStorage.getItem(ENDPOINTS_STORAGE_KEY));
}

function client(): QueryClient {
  return createAppQueryClient();
}

describe('endpoint title sync', () => {
  afterEach(() => {
    window.localStorage.clear();
  });

  it('copies the title of a fetched or patched endpoint into the remembered record', () => {
    window.localStorage.setItem(
      ENDPOINTS_STORAGE_KEY,
      serializeEndpoints([{ id: ID, label: null, added: 1, lastOpened: 1, lastSeenCaptureId: null }]),
    );
    const queryClient = client();

    queryClient.setQueryData(queryKeys.endpoint(ID), { ...endpointFixture, title: 'Stripe test' });
    expect(stored()).toHaveLength(1);
    expect(stored()[0]?.title).toBe('Stripe test');

    queryClient.setQueryData(queryKeys.endpoint(ID), { ...endpointFixture, title: null });
    expect(stored()[0]).not.toHaveProperty('title');
  });

  it('ignores endpoints that are not remembered and other endpoint queries', () => {
    const queryClient = client();
    queryClient.setQueryData(queryKeys.endpoint(ID), { ...endpointFixture, title: 'Stranger' });
    expect(stored()).toEqual([]);

    rememberEndpoint(ID);
    queryClient.setQueryData(['endpoint', ID, 'aliases'], { id: ID, title: 'Not an endpoint' });
    expect(stored()[0]).not.toHaveProperty('title');
  });
});
