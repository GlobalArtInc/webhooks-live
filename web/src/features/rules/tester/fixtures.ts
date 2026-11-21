import type { Rule } from '../../../api/rulesTypes';
import type { Capture } from '../../../api/types';
import type { DraftRuleInput, TesterDraft } from './types';

export const TESTER_ENDPOINT_ID = '0123456789abcdefghjkmnpq';
export const TESTER_ENDPOINT_URL = `https://hooks.example.test/${TESTER_ENDPOINT_ID}`;
export const TESTER_NEW_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f7a99';

const PAID_BODY = '{"type":"payment_intent.succeeded","data":{"object":{"amount":2000,"currency":"usd"}}}';
const HEALTH_BODY = '';
const RULE_TIMESTAMP = '2026-11-21T10:15:30.123Z';

export const TESTER_RULE_IDS = {
  paid: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f7a51',
  reject: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f7a52',
  mail: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f7a53',
} as const;

function byteLength(text: string): number {
  return new TextEncoder().encode(text).byteLength;
}

export const TESTER_PAID_CAPTURE: Capture = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f7a01',
  seq: 2,
  endpoint_id: TESTER_ENDPOINT_ID,
  received_at: '2026-11-21T10:16:02.511Z',
  method: 'POST',
  path: '/stripe',
  query_string: 'source=demo',
  client_ip: '203.0.113.7',
  content_type: 'application/json',
  body_size: byteLength(PAID_BODY),
  status_override: null,
  response_status: 200,
  forward_state: null,
  url: `${TESTER_ENDPOINT_URL}/stripe?source=demo`,
  host: 'hooks.example.test',
  headers: [
    { name: 'Content-Type', value: 'application/json' },
    { name: 'X-Demo-Signature', value: 'demo-signature-value' },
  ],
  query: [{ name: 'source', value: 'demo' }],
  form: null,
  body: { encoding: 'utf8', data: PAID_BODY, truncated: false },
  forwards: [],
};

export const TESTER_HEALTH_CAPTURE: Capture = {
  id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f7a02',
  seq: 1,
  endpoint_id: TESTER_ENDPOINT_ID,
  received_at: '2026-11-21T10:15:40.100Z',
  method: 'GET',
  path: '/health',
  query_string: '',
  client_ip: '203.0.113.8',
  content_type: null,
  body_size: 0,
  status_override: null,
  response_status: 200,
  forward_state: null,
  url: `${TESTER_ENDPOINT_URL}/health`,
  host: 'hooks.example.test',
  headers: [{ name: 'User-Agent', value: 'demo-probe' }],
  query: [],
  form: null,
  body: { encoding: 'utf8', data: HEALTH_BODY, truncated: false },
  forwards: [],
};

export const TESTER_CAPTURES: Capture[] = [TESTER_PAID_CAPTURE, TESTER_HEALTH_CAPTURE];

export const TESTER_DRAFT_INPUT: DraftRuleInput = {
  name: 'Mark paid orders',
  enabled: true,
  on_match: 'next',
  condition: {
    all: [
      { value: 'request.header.x-demo-signature', op: 'exists', arg: null },
      { value: 'request.json.type', op: 'eq', arg: 'payment_intent.succeeded' },
    ],
  },
  steps: [
    { kind: 'extract', from: 'request.json', json_path: '$.data.object.amount', into: 'amount' },
    { kind: 'respond', status: '200', content_type: 'application/json', headers: [], body: '{"ok":true}' },
    { kind: 'http', method: 'POST', url: 'https://erp.example.test/paid', headers: [], body: null },
    { kind: 'increment', counter: 'paid', by: 1 },
  ],
};

export const TESTER_DRAFT: TesterDraft = { ruleId: TESTER_RULE_IDS.paid, input: TESTER_DRAFT_INPUT };

interface SavedRule extends Rule {
  on_match: 'next' | 'stop';
}

function savedRule(id: string, position: number, input: DraftRuleInput, onMatch: 'next' | 'stop'): SavedRule {
  return {
    id,
    name: typeof input.name === 'string' ? input.name : '',
    enabled: true,
    condition: {},
    steps: [],
    position,
    created_at: RULE_TIMESTAMP,
    updated_at: RULE_TIMESTAMP,
    on_match: onMatch,
    ...input,
  };
}

export const TESTER_SAVED_RULES: Rule[] = [
  savedRule(TESTER_RULE_IDS.paid, 1, TESTER_DRAFT_INPUT, 'next'),
  savedRule(
    TESTER_RULE_IDS.reject,
    2,
    {
      name: 'Reject unsigned calls',
      condition: { value: 'request.method', op: 'eq', arg: 'POST' },
      steps: [{ kind: 'respond', status: '401', content_type: 'text/plain', headers: [], body: 'Unauthorized' }],
    },
    'stop',
  ),
  savedRule(
    TESTER_RULE_IDS.mail,
    3,
    {
      name: 'Mail to chat',
      condition: {},
      steps: [{ kind: 'notify', channel_id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f7a60', message: null }],
    },
    'next',
  ),
];
