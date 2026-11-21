import type { Page, Route } from '@playwright/test';
import type { SendRequest } from '../../src/api/types';
import { simulateDryRun } from '../../src/features/rules/tester/fixtureEngine';
import { TESTER_CAPTURES, TESTER_NEW_CAPTURE_ID, TESTER_SAVED_RULES } from '../../src/features/rules/tester/fixtures';
import type { DraftRuleInput } from '../../src/features/rules/tester/types';

export const SLOW_MARKER = 'slowmark';

const SLOW_DELAY_MS = 1500;

interface DryRunBody {
  capture_id: string;
  rules: DraftRuleInput[];
  execute_http: boolean;
}

export interface TesterApiState {
  dryRuns: DryRunBody[];
  released: string[];
  sends: SendRequest[];
}

function json(route: Route, status: number, body: unknown): Promise<void> {
  return route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
}

function mentions(rules: readonly DraftRuleInput[], marker: string): boolean {
  return JSON.stringify(rules).includes(marker);
}

async function answerDryRun(route: Route, state: TesterApiState): Promise<void> {
  const body = route.request().postDataJSON() as DryRunBody;
  state.dryRuns.push(body);
  const capture = TESTER_CAPTURES.find((candidate) => candidate.id === body.capture_id);
  if (capture === undefined) {
    await json(route, 404, { error: { code: 'capture_not_found', message: 'Capture not found.' } });
    return;
  }
  const answer = simulateDryRun(body.rules, capture, { extended: true });
  if (mentions(body.rules, SLOW_MARKER)) {
    await new Promise((resolve) => setTimeout(resolve, SLOW_DELAY_MS));
    state.released.push(SLOW_MARKER);
  }
  await json(route, 200, answer).catch(() => undefined);
}

async function answerSend(route: Route, state: TesterApiState): Promise<void> {
  state.sends.push(route.request().postDataJSON() as SendRequest);
  await json(route, 200, {
    id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f7b01',
    status: 200,
    headers: [],
    body: null,
    duration_ms: 12,
    final_url: null,
    error: null,
    capture_id: TESTER_NEW_CAPTURE_ID,
  });
}

export async function mockTesterApi(page: Page): Promise<TesterApiState> {
  const state: TesterApiState = { dryRuns: [], released: [], sends: [] };

  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname.replace('/api/v1', '');
    const method = request.method();

    if (method === 'GET' && /^\/endpoints\/[^/]+\/captures$/.test(path)) {
      await json(route, 200, { items: TESTER_CAPTURES, next_cursor: null });
      return;
    }
    const detail = /^\/endpoints\/[^/]+\/captures\/([^/]+)$/.exec(path);
    if (method === 'GET' && detail !== null) {
      const capture = TESTER_CAPTURES.find((candidate) => candidate.id === detail[1]);
      await (capture === undefined
        ? json(route, 404, { error: { code: 'capture_not_found', message: 'Capture not found.' } })
        : json(route, 200, capture));
      return;
    }
    if (method === 'GET' && /^\/endpoints\/[^/]+\/rules$/.test(path)) {
      await json(route, 200, { items: TESTER_SAVED_RULES, version: 1 });
      return;
    }
    if (method === 'POST' && /^\/endpoints\/[^/]+\/rules\/dry-run$/.test(path)) {
      await answerDryRun(route, state);
      return;
    }
    if (method === 'POST' && path === '/send') {
      await answerSend(route, state);
      return;
    }
    await json(route, 404, { error: { code: 'not_mocked', message: path } });
  });

  return state;
}
