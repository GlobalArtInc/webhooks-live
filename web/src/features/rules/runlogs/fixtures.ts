import type { RuleRun, RuleRunEntry } from '../../../api/ruleRunTypes';

export const RUN_CAPTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a01';
export const RUN_ENDPOINT_ID = '0123456789abcdefghjkmnpq';
export const RULE_PAID_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5c01';
export const RULE_BAD_SIGNATURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f5c02';

export const RUN_FIXTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f6a01';
export const RERUN_FIXTURE_ID = '0193a1c2-9d10-7a3e-8b44-1c2d3e4f6a02';

const CAPTURE_TIME = '2026-11-21T10:00:00.000Z';
const RERUN_TIME = '2026-11-21T10:05:00.000Z';

const CAPTURE_RUN_ENTRIES: RuleRunEntry[] = [
  { rule_id: RULE_PAID_ID, rule_name: 'Mark paid orders', matched: true },
  {
    rule_id: RULE_PAID_ID,
    step: 0,
    name: 'paid_at',
    kind: 'set_var',
    outcome: 'ok',
    duration_ms: 1,
    detail: null,
    output: null,
  },
  {
    rule_id: RULE_PAID_ID,
    step: 1,
    name: null,
    kind: 'respond',
    outcome: 'ok',
    duration_ms: 2,
    detail: null,
    output: {
      status: 200,
      headers: [{ name: 'X-Api-Key', value: '[redacted]' }],
      body: '{"status":"paid"}',
    },
  },
  {
    rule_id: RULE_PAID_ID,
    step: 2,
    name: 'notify_chat',
    kind: 'http',
    outcome: 'failed',
    duration_ms: 5000,
    detail: 'timeout after 5 s',
    output: null,
  },
  { rule_id: RULE_BAD_SIGNATURE_ID, rule_name: 'Reject bad signature', matched: false },
];

export function ruleRunFixture(overrides: Partial<RuleRun> = {}): RuleRun {
  return {
    id: RUN_FIXTURE_ID,
    capture_id: RUN_CAPTURE_ID,
    trigger: 'capture',
    state: 'succeeded',
    rules_version: 3,
    response_status: 200,
    error: null,
    entries: CAPTURE_RUN_ENTRIES,
    created_at: CAPTURE_TIME,
    finished_at: CAPTURE_TIME,
    ...overrides,
  };
}

export const rerunFailedFixture: RuleRun = ruleRunFixture({
  id: RERUN_FIXTURE_ID,
  trigger: 'rerun',
  state: 'failed',
  response_status: null,
  error: 'http: timeout after 5 s',
  entries: [
    { rule_id: RULE_PAID_ID, rule_name: 'Mark paid orders', matched: true },
    {
      rule_id: RULE_PAID_ID,
      step: 2,
      name: 'notify_chat',
      kind: 'http',
      outcome: 'failed',
      duration_ms: 5000,
      detail: 'timeout after 5 s',
      output: null,
    },
  ],
  created_at: RERUN_TIME,
  finished_at: RERUN_TIME,
});

export const captureRunsFixture: RuleRun[] = [rerunFailedFixture, ruleRunFixture()];
