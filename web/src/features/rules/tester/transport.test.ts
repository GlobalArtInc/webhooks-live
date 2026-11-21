import { afterEach, describe, expect, it } from 'vitest';
import { ApiError } from '../../../api/client';
import { endpointFixture } from '../../../test/fixtures/endpoint';
import { mockServer, registerMockServer } from '../../../test/msw/server';
import { seedEndpoint } from '../../../test/msw/store';
import { testerHandlers } from '../../../test/msw/tester';
import { TESTER_CAPTURES, TESTER_DRAFT_INPUT, TESTER_ENDPOINT_ID, TESTER_PAID_CAPTURE } from './fixtures';
import { httpTesterTransport } from './transport';

registerMockServer();

afterEach(() => {
  mockServer.resetHandlers();
});

function seed(): void {
  mockServer.use(...testerHandlers);
  seedEndpoint({ ...endpointFixture, id: TESTER_ENDPOINT_ID }, TESTER_CAPTURES);
}

describe('httpTesterTransport', () => {
  it('reads the dry run answer of the rules contract', async () => {
    seed();

    const run = await httpTesterTransport.dryRun(
      TESTER_ENDPOINT_ID,
      { capture_id: TESTER_PAID_CAPTURE.id, rules: [TESTER_DRAFT_INPUT], execute_http: false },
      new AbortController().signal,
    );

    expect(run.state).toBe('succeeded');
    expect(run.responseStatus).toBe(200);
    expect(run.response).toBeNull();
    expect(run.entries.map((entry) => (entry.type === 'rule' ? 'rule' : entry.outcome))).toEqual([
      'rule',
      'ok',
      'ok',
      'deferred',
      'no_op',
    ]);
  });

  it('turns an invalid draft into field errors with their paths', async () => {
    seed();

    const failure = await httpTesterTransport
      .dryRun(
        TESTER_ENDPOINT_ID,
        {
          capture_id: TESTER_PAID_CAPTURE.id,
          rules: [{ name: 'x', steps: [{ kind: 'teleport' }] }],
          execute_http: false,
        },
        new AbortController().signal,
      )
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(ApiError);
    expect(failure instanceof ApiError && failure.status).toBe(422);
    expect(failure instanceof ApiError && failure.details[0]?.field).toBe('rules.0.steps.0.kind');
  });

  it('reports an unknown capture as not found', async () => {
    seed();

    const failure = await httpTesterTransport
      .dryRun(
        TESTER_ENDPOINT_ID,
        { capture_id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f7aff', rules: [], execute_http: false },
        new AbortController().signal,
      )
      .catch((error: unknown) => error);

    expect(failure instanceof ApiError && failure.status).toBe(404);
  });

  it('lists the newest captures first', async () => {
    seed();

    const captures = await httpTesterTransport.listCaptures(TESTER_ENDPOINT_ID);

    expect(captures.map((capture) => capture.seq)).toEqual([2, 1]);
  });
});
