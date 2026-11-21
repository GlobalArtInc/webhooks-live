import { http, type HttpHandler } from 'msw';
import type { RuleRun, RuleRunPage, RuleRunState } from '../../api/ruleRunTypes';
import { errorResponse, jsonResponse } from './responses';
import { randomUuid } from './store';

interface EndpointPath {
  endpointId: string;
}

interface CapturePath extends EndpointPath {
  captureId: string;
}

const RUNS = '/api/v1/endpoints/:endpointId/rule-runs';
const CAPTURE_RUNS = '/api/v1/endpoints/:endpointId/captures/:captureId/rule-runs';
const DEFAULT_LIMIT = 50;
const MAX_LIMIT = 200;
const RERUN_LIMIT = 10;
const ACTIVE_STATES: readonly RuleRunState[] = ['sync', 'queued', 'running'];
const STATES: readonly string[] = ['sync', 'queued', 'running', 'succeeded', 'failed'];

const runsByEndpoint = new Map<string, RuleRun[]>();

export function seedRuleRuns(endpointId: string, runs: RuleRun[]): void {
  runsByEndpoint.set(endpointId, [...runs]);
}

export function resetRuleRunStore(): void {
  runsByEndpoint.clear();
}

function runsOf(endpointId: string): RuleRun[] {
  return runsByEndpoint.get(endpointId) ?? [];
}

function isActive(run: RuleRun): boolean {
  return ACTIVE_STATES.includes(run.state);
}

function limitOf(raw: string | null): number | null {
  const limit = raw === null ? DEFAULT_LIMIT : Number(raw);
  return Number.isInteger(limit) && limit >= 1 && limit <= MAX_LIMIT ? limit : null;
}

function listRuns(endpointId: string, url: URL): Response {
  const state = url.searchParams.get('state');
  if (state !== null && !STATES.includes(state)) {
    return errorResponse(422, 'validation_failed', 'state must be sync, queued, running, succeeded or failed.', [
      { field: 'state', code: 'invalid_format', message: 'must be sync, queued, running, succeeded or failed' },
    ]);
  }
  const limit = limitOf(url.searchParams.get('limit'));
  if (limit === null) {
    return errorResponse(422, 'validation_failed', 'limit must be between 1 and 200.', [
      { field: 'limit', code: 'out_of_range', message: 'must be between 1 and 200' },
    ]);
  }
  const matching = runsOf(endpointId).filter((run) => state === null || run.state === state);
  const cursor = url.searchParams.get('cursor');
  const start = cursor === null ? 0 : matching.findIndex((run) => run.id === cursor) + 1;
  if (start === 0 && cursor !== null) {
    return errorResponse(400, 'invalid_cursor', 'the cursor is malformed or does not match the order and filters.');
  }
  const items = matching.slice(start, start + limit);
  const more = start + limit < matching.length;
  const page: RuleRunPage = { items, next_cursor: more ? (items.at(-1)?.id ?? null) : null };
  return jsonResponse(page);
}

function createRerun(params: CapturePath): Response {
  const runs = runsOf(params.endpointId);
  const captureRuns = runs.filter((run) => run.capture_id === params.captureId);
  if (captureRuns.some(isActive)) {
    return errorResponse(409, 'rerun_active', 'a re-run of this capture is already queued or running.');
  }
  if (captureRuns.filter((run) => run.trigger === 'rerun').length >= RERUN_LIMIT) {
    return jsonResponse(
      {
        error: {
          code: 'limit_reached',
          message: 'this capture already has the maximum number of re-runs.',
          limit: { name: 'rule_reruns', max: RERUN_LIMIT, window: null, resets_at: null },
        },
      },
      422,
    );
  }
  const created: RuleRun = {
    id: randomUuid(),
    capture_id: params.captureId,
    trigger: 'rerun',
    state: 'queued',
    rules_version: 1,
    response_status: null,
    error: null,
    entries: [],
    created_at: new Date().toISOString(),
    finished_at: null,
  };
  seedRuleRuns(params.endpointId, [created, ...runs]);
  return jsonResponse(created, 202);
}

export const ruleRunHandlers: HttpHandler[] = [
  http.get<EndpointPath>(RUNS, ({ params, request }) => listRuns(params.endpointId, new URL(request.url))),
  http.post<CapturePath>(CAPTURE_RUNS, ({ params }) => createRerun(params)),
];
