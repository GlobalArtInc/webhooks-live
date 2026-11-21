import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it } from 'vitest';
import { createI18nInstance } from '../../../i18n/setup';
import { registerMockServer } from '../../../test/msw/server';
import { seedRuleRuns } from '../../../test/msw/runLogs';
import { CaptureRulesSection, rerunFailureOf } from './CaptureRulesSection';
import { captureRunsFixture, RUN_CAPTURE_ID, RUN_ENDPOINT_ID, ruleRunFixture } from './fixtures';
import type { RuleRun } from '../../../api/ruleRunTypes';
import { ApiError } from '../../../api/client';

registerMockServer();

afterEach(() => {
  cleanup();
});

function queuedRerun(id: string): RuleRun {
  return ruleRunFixture({ id, trigger: 'rerun', state: 'queued', response_status: null, entries: [] });
}

function renderSection(runs: RuleRun[], canRerun: boolean) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <QueryClientProvider client={queryClient}>
        <CaptureRulesSection endpointId={RUN_ENDPOINT_ID} captureId={RUN_CAPTURE_ID} runs={runs} canRerun={canRerun} />
      </QueryClientProvider>
    </I18nextProvider>,
  );
}

async function confirmRerun(): Promise<void> {
  fireEvent.click(screen.getByRole('button', { name: 'Run again' }));
  const dialog = await screen.findByRole('dialog');
  fireEvent.click(within(dialog).getByRole('button', { name: 'Run again' }));
}

describe('CaptureRulesSection', () => {
  it('shows each rule with its steps and the redacted output as text, without a rerun for viewers', () => {
    renderSection(captureRunsFixture, false);

    expect(screen.getAllByText('Mark paid orders').length).toBeGreaterThan(0);
    expect(screen.getAllByText('timeout after 5 s').length).toBeGreaterThan(0);
    expect(screen.getAllByText(/\[redacted\]/).length).toBeGreaterThan(0);
    expect(screen.queryByRole('button', { name: 'Run again' })).toBeNull();
  });

  it('reports a rerun that is already active', async () => {
    seedRuleRuns(RUN_ENDPOINT_ID, [queuedRerun('0193a1c2-9d10-7a3e-8b44-1c2d3e4f6c01')]);
    renderSection(captureRunsFixture, true);

    await confirmRerun();

    expect((await screen.findByRole('alert')).textContent).toContain(
      'A re-run of this capture is already queued or running.',
    );
  });

  it('reports the re-run limit with the maximum from the server', async () => {
    const reruns = Array.from({ length: 10 }, (_value, index) =>
      ruleRunFixture({
        id: `0193a1c2-9d10-7a3e-8b44-1c2d3e4f7${String(index).padStart(3, '0')}`,
        trigger: 'rerun',
        state: 'succeeded',
      }),
    );
    seedRuleRuns(RUN_ENDPOINT_ID, reruns);
    renderSection(reruns, true);

    await confirmRerun();

    expect((await screen.findByRole('alert')).textContent).toContain('maximum of 10 re-runs.');
  });

  it('confirms a queued re-run', async () => {
    seedRuleRuns(RUN_ENDPOINT_ID, []);
    renderSection(captureRunsFixture, true);

    await confirmRerun();

    await waitFor(() => expect(screen.getByRole('status').textContent).toContain('Re-run queued.'));
  });
});

describe('rerunFailureOf', () => {
  it('maps a conflict and a limit to their own messages and anything else to a generic failure', () => {
    const conflict = new ApiError({ status: 409, code: 'rerun_active', message: '', details: [], retryAfter: null });
    const limit = new ApiError({
      status: 422,
      code: 'limit_reached',
      message: '',
      details: [],
      retryAfter: null,
      limit: { name: 'rule_reruns', max: 10, window: null, resets_at: null },
    });

    expect(rerunFailureOf(conflict)).toEqual({ kind: 'active', max: 0 });
    expect(rerunFailureOf(limit)).toEqual({ kind: 'limit', max: 10 });
    expect(rerunFailureOf(new Error('network'))).toEqual({ kind: 'failed', max: 0 });
    expect(rerunFailureOf(null)).toBeNull();
  });
});
