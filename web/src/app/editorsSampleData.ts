import type { CaptureSummary } from '../api/types';
import type { CompletionEntry, EditorDiagnostic } from '../features/editor/types';
import type { KeyValueRow } from '../components/KeyValueEditor/rows';
import type { VariableEntry } from '../components/VariablePicker/variables';

export const SAMPLE_NOW_MS = Date.parse('2026-11-21T14:02:11.000Z');

export const SAMPLE_BODY = [
  '{',
  '  "received": "{{ request.json.id }}",',
  '  "amount": {{ request.json.data.amount }},',
  '  "at": "{{ now | isodate }}",',
  '  "note": "{{ request.query.missing }}"',
  '}',
].join('\n');

export const SAMPLE_SCRIPT = [
  '// Reply with the order id, or refuse unknown events',
  'let event = request.json.event;',
  'if event == "paid" {',
  '  respond(200, #{ ok: true, id: request.json.id });',
  '} else {',
  '  respond(400, "unknown event");',
  '}',
  'let total = request.json.data.amount +',
].join('\n');

export const SAMPLE_READ_ONLY_BODY = '{\n  "order": 42,\n  "status": "paid"\n}';

export const SAMPLE_HEADER_ROWS: readonly KeyValueRow[] = [
  { id: 'sample-row-1', name: 'x-order-id', value: '{{ request.json.data.id }}' },
  { id: 'sample-row-2', name: 'x mode', value: '{{ request.query.mode }}' },
  { id: 'sample-row-3', name: 'x-trace', value: 'static' },
];

export const SAMPLE_SENDERS: readonly string[] = ['203.0.113.0/24', '2001:db8::/32', '300.1.1.1/8'];

export const SAMPLE_SENDER_SUGGESTIONS: readonly string[] = ['10.0.0.0/8', '192.168.0.0/16', '127.0.0.1/32'];

export const VARIABLE_CATALOGUE: readonly VariableEntry[] = [
  { path: 'method', type: 'string', sample: '"POST"', insertText: '{{ request.method }}', group: 'request' },
  { path: 'path', type: 'string', sample: '"/stripe"', insertText: '{{ request.path }}', group: 'request' },
  { path: 'query.mode', type: 'string', sample: '"test"', insertText: '{{ request.query.mode }}', group: 'request' },
  {
    path: 'header.stripe-signature',
    type: 'string',
    sample: '"t=1732...,v1=9a..."',
    insertText: '{{ request.header.stripe-signature }}',
    group: 'request',
  },
  { path: 'json.id', type: 'string', sample: '"evt_1Q"', insertText: '{{ request.json.id }}', group: 'request' },
  {
    path: 'json.data.amount',
    type: 'number',
    sample: '2000',
    insertText: '{{ request.json.data.amount }}',
    group: 'request',
  },
  { path: 'now', type: 'time', sample: '2026-11-21T14:02:11Z', insertText: '{{ now }}', group: 'time' },
  {
    path: 'vars.amount',
    description: 'set by rule 1, step 1',
    insertText: '{{ vars.amount }}',
    group: 'rules',
  },
  { path: 'uuid', description: 'a new random UUID', insertText: '{{ uuid }}', group: 'helpers' },
  { path: 'custom.thing', sample: '"x"', insertText: '{{ custom.thing }}', group: 'extras' },
];

function summary(seq: number, method: string, path: string, secondsAgo: number): CaptureSummary {
  return {
    id: `00000000-0000-4000-8000-${seq.toString(16).padStart(12, '0')}`,
    seq,
    endpoint_id: '0123456789abcdefghjkmnpq',
    received_at: new Date(SAMPLE_NOW_MS - secondsAgo * 1000).toISOString(),
    method,
    path,
    query_string: '',
    client_ip: '203.0.113.7',
    content_type: 'application/json',
    body_size: 120,
    status_override: null,
    response_status: 200,
    forward_state: null,
  };
}

export const SAMPLE_CAPTURES: readonly CaptureSummary[] = [
  summary(3, 'POST', '/stripe', 12),
  summary(2, 'GET', '/health', 95),
  summary(1, 'PUT', '/orders/42', 3700),
];

export function templateDiagnostics(text: string): EditorDiagnostic[] {
  const found: EditorDiagnostic[] = [];
  text.split('\n').forEach((line, index) => {
    const filter = line.indexOf('| isodate');
    if (filter !== -1) {
      found.push({
        line: index + 1,
        column: filter + 3,
        length: 7,
        code: 'unknown_filter',
        message: 'Unknown filter "isodate"',
      });
    }
    const missing = line.indexOf('request.query.missing');
    if (missing !== -1) {
      found.push({
        line: index + 1,
        column: missing + 1,
        length: 'request.query.missing'.length,
        code: 'unknown_variable',
        message: 'Unknown variable renders empty',
        severity: 'warning',
      });
    }
  });
  return found;
}

export function scriptDiagnostics(text: string): EditorDiagnostic[] {
  const lines = text.split('\n');
  const last = lines.length;
  const tail = lines[last - 1] ?? '';
  if (tail.trimEnd().endsWith('+')) {
    return [
      {
        line: last,
        column: tail.length,
        length: 1,
        code: 'script_invalid',
        message: 'Expecting an expression after the operator',
      },
    ];
  }
  return [];
}

export const SCRIPT_COMPLETIONS: readonly CompletionEntry[] = [
  {
    label: 'respond',
    insertText: 'respond(',
    detail: 'fn(status, body)',
    description: 'Send the response',
    kind: 'function',
  },
  { label: 'request', detail: 'map', description: 'The captured request', kind: 'variable' },
  { label: 'log', insertText: 'log(', detail: 'fn(text)', description: 'Write to the run log', kind: 'function' },
  { label: 'let', kind: 'keyword' },
];
