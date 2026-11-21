import { cleanup, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { renderAppAt } from '../endpoints/renderApp';
import { registerMockServer } from '../../test/msw/server';
import { mockStore } from '../../test/msw/store';
import { usageStore, type UsageFailure } from '../../test/msw/usage';
import { instanceFixture } from '../../test/fixtures/instance';
import {
  SIGNED_IN_INSTANCE,
  fullOutboundMeter,
  meterFixture,
  nearStorageMeter,
  usageFixture,
} from '../../test/fixtures/usage';

registerMockServer();

afterEach(() => {
  cleanup();
});

const UNAVAILABLE_FAILURES: UsageFailure[] = [
  { status: 401, body: { error: { code: 'unauthenticated', message: 'Sign in to continue.' } } },
  { status: 403, body: { error: { code: 'insufficient_scope', message: 'Token scope too narrow.' } } },
];

describe('usage page', () => {
  it('shows the value, window, reset time and the at-limit sentence of each meter', async () => {
    mockStore.instance = SIGNED_IN_INSTANCE;
    usageStore.usage = usageFixture([nearStorageMeter(), fullOutboundMeter(), meterFixture()]);
    renderAppAt('/usage');

    const storage = await screen.findByRole('progressbar', { name: 'Storage' });
    expect(screen.getByRole('heading', { level: 1, name: 'Usage and limits' })).toBeTruthy();
    expect(screen.getByText('Tier member')).toBeTruthy();

    expect(storage.getAttribute('aria-valuenow')).toBe('900000000');
    expect(screen.getByText('90% used. Close to the limit.')).toBeTruthy();

    const outbound = screen.getByRole('progressbar', { name: 'Outbound requests' });
    expect(outbound.getAttribute('aria-valuetext')).toBe('200 of 200');
    expect(screen.getByText(/^At the limit\. New items are refused until it resets in /)).toBeTruthy();
    expect(screen.getByText('Per hour')).toBeTruthy();
    expect(screen.getAllByText(/used\. Close to the limit\.|At the limit\./)).toHaveLength(2);
  });

  it('shows the not-available page when the instance has no sign-in', async () => {
    mockStore.instance = instanceFixture;
    renderAppAt('/usage');

    expect(
      await screen.findByRole('heading', { level: 1, name: 'Usage and limits are not available here' }),
    ).toBeTruthy();
  });

  it.each(UNAVAILABLE_FAILURES)('shows the not-available page when usage answers $status', async (failure) => {
    mockStore.instance = SIGNED_IN_INSTANCE;
    usageStore.failure = failure;
    renderAppAt('/usage');

    expect(
      await screen.findByRole('heading', { level: 1, name: 'Usage and limits are not available here' }),
    ).toBeTruthy();
  });

  it('shows the warning sentence in the header when a meter reaches 90 percent', async () => {
    mockStore.instance = SIGNED_IN_INSTANCE;
    usageStore.usage = usageFixture([nearStorageMeter()]);
    renderAppAt('/settings');

    const link = await screen.findByRole('link', { name: /Storage: 90% of its limit/ });
    expect(link.getAttribute('href')).toBe('/usage');
  });

  it('asks for usage in the header only when the user is signed in', async () => {
    mockStore.instance = instanceFixture;
    renderAppAt('/settings');

    await screen.findByRole('banner');
    expect(usageStore.requests).toBe(0);
  });

  it('refetches usage when the window regains focus', async () => {
    mockStore.instance = SIGNED_IN_INSTANCE;
    renderAppAt('/usage');
    await screen.findByRole('progressbar', { name: 'Endpoints' });
    const before = usageStore.requests;

    window.dispatchEvent(new Event('visibilitychange'));

    await waitFor(() => expect(usageStore.requests).toBeGreaterThan(before));
  });
});
