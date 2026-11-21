import { QueryClient } from '@tanstack/react-query';
import { describe, expect, it } from 'vitest';
import type { Capture } from '../../../api/types';
import type { LiveEvent } from '../../live/types';
import { parseLiveEvent } from '../../live/parse';
import { applyLiveEventToCache } from '../../live/applyLiveEvent';
import { queryKeys } from '../../../api/queries';
import { jsonCaptureFixture } from '../../../test/fixtures/captures';
import { ruleRunKeys } from './api';
import { RUN_CAPTURE_ID, RUN_ENDPOINT_ID, ruleRunFixture } from './fixtures';
import type { RuleRun } from '../../../api/ruleRunTypes';

const queued: RuleRun = ruleRunFixture({
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f6b01',
  trigger: 'rerun',
  state: 'queued',
  entries: [],
});
const captureDetail: Capture = {
  ...jsonCaptureFixture,
  id: RUN_CAPTURE_ID,
  endpoint_id: RUN_ENDPOINT_ID,
  rule_runs: [queued],
};

function resultOf(run: RuleRun): LiveEvent {
  return { name: 'rule_result', data: { capture_id: RUN_CAPTURE_ID, run } };
}

function seeded(): QueryClient {
  const queryClient = new QueryClient();
  queryClient.setQueryData(queryKeys.capture(RUN_ENDPOINT_ID, RUN_CAPTURE_ID), captureDetail);
  queryClient.setQueryData(ruleRunKeys.list(RUN_ENDPOINT_ID, {}), {
    pages: [{ items: [queued], next_cursor: null }],
    pageParams: [null],
  });
  return queryClient;
}

describe('rule_result live events', () => {
  it('replaces the run in the capture detail and in the run lists', () => {
    const queryClient = seeded();
    const finished = { ...queued, state: 'succeeded' as const, entries: ruleRunFixture().entries };

    applyLiveEventToCache(queryClient, RUN_ENDPOINT_ID, resultOf(finished));

    const detail = queryClient.getQueryData<Capture>(queryKeys.capture(RUN_ENDPOINT_ID, RUN_CAPTURE_ID));
    const list = queryClient.getQueryData<{ pages: { items: RuleRun[] }[] }>(ruleRunKeys.list(RUN_ENDPOINT_ID, {}));
    expect(detail?.rule_runs?.map((run) => run.state)).toEqual(['succeeded']);
    expect(list?.pages[0]?.items.map((run) => run.state)).toEqual(['succeeded']);
  });

  it('removes a run from a failed-only list when it finishes successfully', () => {
    const queryClient = new QueryClient();
    const failedFilter = { state: 'failed' as const };
    const failed = ruleRunFixture({ id: queued.id, state: 'failed', error: 'http: timeout after 5 s' });
    queryClient.setQueryData(ruleRunKeys.list(RUN_ENDPOINT_ID, failedFilter), {
      pages: [{ items: [failed], next_cursor: null }],
      pageParams: [null],
    });

    applyLiveEventToCache(
      queryClient,
      RUN_ENDPOINT_ID,
      resultOf(ruleRunFixture({ id: queued.id, state: 'succeeded' })),
    );

    const list = queryClient.getQueryData<{ pages: { items: RuleRun[] }[] }>(
      ruleRunKeys.list(RUN_ENDPOINT_ID, failedFilter),
    );
    expect(list?.pages[0]?.items).toEqual([]);
  });

  it('accepts a well-formed payload and rejects a run with an unknown state', () => {
    const payload = { capture_id: RUN_CAPTURE_ID, run: queued };

    expect(parseLiveEvent('rule_result', JSON.stringify(payload))?.name).toBe('rule_result');
    expect(parseLiveEvent('rule_result', JSON.stringify({ ...payload, run: { ...queued, state: 'done' } }))).toBeNull();
  });
});
