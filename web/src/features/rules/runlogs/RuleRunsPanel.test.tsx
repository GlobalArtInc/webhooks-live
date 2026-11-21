import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it } from 'vitest';
import { createI18nInstance } from '../../../i18n/setup';
import { registerMockServer } from '../../../test/msw/server';
import { seedRuleRuns } from '../../../test/msw/runLogs';
import { RuleRunsPanel } from './RuleRunsPanel';
import { RUN_ENDPOINT_ID, rerunFailedFixture, ruleRunFixture } from './fixtures';
import type { RuleRun } from '../../../api/ruleRunTypes';

registerMockServer();

afterEach(() => {
  cleanup();
});

function renderPanel() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <QueryClientProvider client={queryClient}>
        <RuleRunsPanel endpointId={RUN_ENDPOINT_ID} />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

function numberedRuns(count: number): RuleRun[] {
  return Array.from({ length: count }, (_value, index) =>
    ruleRunFixture({
      id: `0193a1c2-9d10-7a3e-8b44-1c2d3e4f8${String(index).padStart(3, '0')}`,
      entries: [],
      trigger: 'capture',
    }),
  );
}

describe('RuleRunsPanel', () => {
  it('narrows the list to failed runs when the filter is on', async () => {
    seedRuleRuns(RUN_ENDPOINT_ID, [rerunFailedFixture, ruleRunFixture()]);
    renderPanel();
    expect(await screen.findByText('On capture')).not.toBeNull();

    fireEvent.click(screen.getByRole('checkbox', { name: 'Failed only' }));

    await waitFor(() => expect(screen.queryByText('On capture')).toBeNull());
    expect(await screen.findByText('Re-run')).not.toBeNull();
  });

  it('loads the next page with the cursor from the server', async () => {
    seedRuleRuns(RUN_ENDPOINT_ID, numberedRuns(51));
    renderPanel();
    const loadMore = await screen.findByRole('button', { name: 'Load more runs' });

    fireEvent.click(loadMore);

    await waitFor(() => expect(screen.queryByRole('button', { name: 'Load more runs' })).toBeNull());
    expect(screen.getAllByText('On capture')).toHaveLength(51);
  });
});
