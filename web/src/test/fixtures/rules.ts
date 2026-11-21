import type { Rule } from '../../api/rulesTypes';

export const RULE_IDS = {
  markPaid: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a51',
  reject: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a52',
  mail: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a53',
} as const;

const CHANNEL_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a60';
const TIMESTAMP = '2026-11-21T10:15:30.123Z';

export function rulesFixture(): Rule[] {
  return [
    {
      id: RULE_IDS.markPaid,
      name: 'Mark paid orders',
      enabled: true,
      condition: { all: [{ value: 'request.json.type', op: 'eq', arg: 'payment_intent.succeeded' }] },
      steps: [
        { kind: 'extract', from: 'request.json', json_path: '$.data.object.amount', into: 'amount' },
        {
          kind: 'respond',
          status: '200',
          content_type: 'application/json',
          headers: [],
          body: '{"ok":true}',
        },
      ],
      position: 1,
      created_at: TIMESTAMP,
      updated_at: TIMESTAMP,
    },
    {
      id: RULE_IDS.reject,
      name: 'Reject bad signature',
      enabled: true,
      condition: { all: [{ value: 'request.header.x-signature', op: 'exists', arg: null }] },
      steps: [{ kind: 'script', source: 'let status = 401;' }],
      position: 2,
      created_at: TIMESTAMP,
      updated_at: TIMESTAMP,
    },
    {
      id: RULE_IDS.mail,
      name: 'Mail to chat',
      enabled: false,
      condition: {},
      steps: [{ kind: 'notify', channel_id: CHANNEL_ID, message: null }],
      position: 3,
      created_at: TIMESTAMP,
      updated_at: TIMESTAMP,
    },
  ];
}
