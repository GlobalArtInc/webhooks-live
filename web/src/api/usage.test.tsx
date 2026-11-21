import { QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { ReactNode } from 'react';
import { createAppQueryClient } from '../app/providers';
import { registerMockServer } from '../test/msw/server';
import { usageStore } from '../test/msw/usage';
import { ApiError } from './client';
import { useUsage } from './usage';

registerMockServer();

afterEach(() => {
  cleanup();
});

describe('usage refetch after a limit error', () => {
  it('refetches the usage counters when a mutation fails with a quota error', async () => {
    const queryClient = createAppQueryClient();
    function wrapper({ children }: { children: ReactNode }) {
      return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
    }
    const { result } = renderHook(() => useUsage(true), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const before = usageStore.requests;

    const quota = new ApiError({
      status: 429,
      code: 'quota_exceeded',
      message: 'slow down',
      details: [],
      retryAfter: 60,
      limit: { name: 'outbound', max: 200, window: 'hour', resets_at: null },
    });
    await act(async () => {
      await queryClient
        .getMutationCache()
        .build(queryClient, { mutationFn: () => Promise.reject(quota) })
        .execute(undefined)
        .catch(() => undefined);
    });

    await waitFor(() => expect(usageStore.requests).toBeGreaterThan(before));
  });
});
