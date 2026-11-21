import type { Page, Route } from '@playwright/test';
import type { Rule } from '../../src/api/rulesTypes';
import type { FieldError } from '../../src/api/types';
import { simulateDryRun } from '../../src/features/rules/tester/fixtureEngine';
import { TESTER_CAPTURES } from '../../src/features/rules/tester/fixtures';
import { SCRIPT_META_FIXTURE, fixtureValidate } from '../../src/features/rules/script/fixtures';
import type { DraftRuleInput } from '../../src/features/rules/tester/types';
import { endpointFixture } from '../../src/test/fixtures/endpoint';
import { instanceFixture, PUBLIC_URL } from '../../src/test/fixtures/instance';
import { installFakeStream } from './fakeStream';
import { ENDPOINT_ID } from './mockApi';

export const EDITOR_RULE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f9a51';
export const RECORDED_RUN_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f9b01';
export const SCRIPT_RULE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f9a52';

const STAMP = '2026-11-21T10:15:30.123Z';

interface DryRunBody {
  capture_id: string;
  rules: DraftRuleInput[];
}

export interface PatchRecord {
  ruleId: string;
  body: Record<string, unknown>;
}

export interface RuleEditorApiState {
  rules: Rule[];
  patches: PatchRecord[];
  dryRuns: DryRunBody[];
  rejectNextPatch: FieldError[] | null;
  rename: (ruleId: string, name: string) => void;
}

export function editorRules(): Rule[] {
  return [
    {
      id: EDITOR_RULE_ID,
      name: 'Mark paid orders',
      enabled: true,
      condition: { all: [{ value: 'request.method', op: 'eq', arg: 'POST' }] },
      steps: [
        { kind: 'extract', from: 'request.body', json_path: '$.data.object.amount', into: 'amount' },
        { kind: 'respond', status: '200', content_type: 'application/json', headers: [], body: '{"ok":true}' },
      ],
      position: 1,
      created_at: STAMP,
      updated_at: STAMP,
    },
    {
      id: SCRIPT_RULE_ID,
      name: 'Reject bad signature',
      enabled: true,
      condition: {},
      steps: [{ kind: 'script', source: 'let status = 401;' }],
      position: 2,
      created_at: STAMP,
      updated_at: STAMP,
    },
  ];
}

function recordedRun() {
  return {
    id: RECORDED_RUN_ID,
    capture_id: TESTER_CAPTURES[0]?.id,
    trigger: 'capture',
    state: 'succeeded',
    rules_version: 1,
    response_status: 200,
    error: null,
    entries: [{ rule_id: EDITOR_RULE_ID, rule_name: 'Mark paid orders', matched: true }],
    created_at: STAMP,
    finished_at: STAMP,
  };
}

function json(route: Route, status: number, body: unknown): Promise<void> {
  return route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
}

export async function mockRuleEditorApi(page: Page): Promise<RuleEditorApiState> {
  const state: RuleEditorApiState = {
    rules: editorRules(),
    patches: [],
    dryRuns: [],
    rejectNextPatch: null,
    rename: (ruleId, name) => {
      state.rules = state.rules.map((rule) =>
        rule.id === ruleId ? { ...rule, name, updated_at: new Date().toISOString() } : rule,
      );
    },
  };
  const rulesPath = `/endpoints/${ENDPOINT_ID}/rules`;

  await installFakeStream(page);
  await page.route('**/api/v1/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname.replace('/api/v1', '');
    const method = request.method();

    if (method === 'GET' && path === '/instance') {
      await json(route, 200, instanceFixture);
    } else if (method === 'GET' && path === `/endpoints/${ENDPOINT_ID}`) {
      await json(route, 200, {
        ...endpointFixture,
        id: ENDPOINT_ID,
        url: `${PUBLIC_URL}/${ENDPOINT_ID}`,
        my_role: 'owner',
      });
    } else if (method === 'GET' && path === rulesPath) {
      await json(route, 200, { items: state.rules, version: 1 });
    } else if (method === 'PATCH' && path.startsWith(`${rulesPath}/`)) {
      await answerPatch(route, state, path.slice(rulesPath.length + 1));
    } else if (method === 'POST' && path === `${rulesPath}/dry-run`) {
      await answerDryRun(route, state);
    } else if (method === 'GET' && path === `/endpoints/${ENDPOINT_ID}/captures`) {
      await json(route, 200, { items: TESTER_CAPTURES, next_cursor: null });
    } else if (method === 'GET' && path.startsWith(`/endpoints/${ENDPOINT_ID}/captures/`)) {
      const capture = TESTER_CAPTURES.find((candidate) => path.endsWith(`/${candidate.id}`));
      await (capture === undefined
        ? json(route, 404, { error: { code: 'capture_not_found', message: 'Capture not found.' } })
        : json(route, 200, capture));
    } else if (method === 'GET' && path === `/endpoints/${ENDPOINT_ID}/template/variables`) {
      await json(route, 200, { delimiters: { open: '{{', close: '}}' }, items: [] });
    } else if (method === 'GET' && (path.endsWith('/notifications') || path.endsWith('/secrets'))) {
      await json(route, 200, { items: [] });
    } else if (method === 'GET' && path === `/endpoints/${ENDPOINT_ID}/rule-runs`) {
      await json(route, 200, { items: [recordedRun()], next_cursor: null });
    } else if (method === 'GET' && path === '/script/meta') {
      await json(route, 200, SCRIPT_META_FIXTURE);
    } else if (method === 'POST' && path === '/script/validate') {
      const body = request.postDataJSON() as { source: string };
      await json(route, 200, { diagnostics: fixtureValidate(body.source) });
    } else {
      await json(route, 404, { error: { code: 'not_mocked', message: path } });
    }
  });
  return state;
}

async function answerPatch(route: Route, state: RuleEditorApiState, ruleId: string): Promise<void> {
  const body = route.request().postDataJSON() as Record<string, unknown>;
  state.patches.push({ ruleId, body });
  if (state.rejectNextPatch !== null) {
    const fields = state.rejectNextPatch;
    state.rejectNextPatch = null;
    await json(route, 422, { error: { code: 'validation_failed', message: 'The rule is invalid.', fields } });
    return;
  }
  const current = state.rules.find((rule) => rule.id === ruleId);
  if (current === undefined) {
    await json(route, 404, { error: { code: 'rule_not_found', message: 'Rule not found.' } });
    return;
  }
  const updated: Rule = { ...current, ...body, updated_at: new Date().toISOString() };
  state.rules = state.rules.map((rule) => (rule.id === ruleId ? updated : rule));
  await json(route, 200, updated);
}

async function answerDryRun(route: Route, state: RuleEditorApiState): Promise<void> {
  const body = route.request().postDataJSON() as DryRunBody;
  state.dryRuns.push(body);
  const capture = TESTER_CAPTURES.find((candidate) => candidate.id === body.capture_id);
  if (capture === undefined) {
    await json(route, 404, { error: { code: 'capture_not_found', message: 'Capture not found.' } });
    return;
  }
  await json(route, 200, simulateDryRun(body.rules, capture, { extended: true })).catch(() => undefined);
}
