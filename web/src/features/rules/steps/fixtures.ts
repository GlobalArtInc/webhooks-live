import type { VariableEntry } from '../../../components/VariablePicker/variables';
import type { RuleStep, StepCatalogueEntry, StepEditorContext, StepError } from './types';

const HTTP_METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'].map((method) => ({ value: method, label: method }));

export const STEP_CATALOGUE: readonly StepCatalogueEntry[] = [
  {
    kind: 'respond',
    title: 'Respond',
    description: 'Answer the sender with a status, headers and a body.',
    class: 'response',
    phase: 'before',
    summary: '{status} · {content_type}',
    produces: [],
    fields: [
      { name: 'status', kind: 'template', label: 'Status', required: true, maxLength: 16, defaultValue: '200' },
      { name: 'content_type', kind: 'template', label: 'Content type', nullable: true, maxLength: 128 },
      {
        name: 'headers',
        kind: 'key_value',
        label: 'Response headers',
        valueKind: 'template',
        nameKind: 'header_name',
        maxRows: 20,
      },
      { name: 'body', kind: 'template', label: 'Body', multiline: true, maxBytes: 64 * 1024 },
    ],
  },
  {
    kind: 'set_header',
    title: 'Set header',
    description: 'Add or replace one response header.',
    class: 'response',
    phase: 'before',
    summary: '{name}: {value}',
    produces: [],
    fields: [
      { name: 'name', kind: 'header_name', label: 'Header name', required: true, maxLength: 64 },
      { name: 'value', kind: 'template', label: 'Value', required: true, maxLength: 1024 },
    ],
  },
  {
    kind: 'delay',
    title: 'Delay',
    description: 'Wait before the next step.',
    class: 'response',
    phase: 'before',
    summary: 'wait {ms} ms',
    produces: [],
    fields: [{ name: 'ms', kind: 'duration', label: 'Delay', required: true, min: 0, max: 10000, defaultValue: 500 }],
  },
  {
    kind: 'stop',
    title: 'Stop',
    description: 'Skip the remaining steps of this rule.',
    class: 'response',
    phase: 'any',
    summary: null,
    produces: [],
    fields: [],
  },
  {
    kind: 'extract',
    title: 'Extract value',
    description: 'Copy a value out of the request into a variable.',
    class: 'compute',
    phase: 'any',
    summary: 'from {from} into {into}',
    produces: ['var.{into}'],
    fields: [
      { name: 'from', kind: 'text', label: 'From', required: true, placeholder: 'request.body', maxLength: 128 },
      { name: 'json_path', kind: 'json_path', label: 'JSON path', nullable: true, placeholder: '$.data.amount' },
      { name: 'regex', kind: 'text', label: 'Regular expression', nullable: true, maxLength: 256 },
      { name: 'into', kind: 'text', label: 'Into variable', required: true, maxLength: 64 },
    ],
  },
  {
    kind: 'set_var',
    title: 'Set variable',
    description: 'Store a rendered template in a variable.',
    class: 'compute',
    phase: 'any',
    summary: '{into} = {value}',
    produces: ['var.{into}'],
    fields: [
      { name: 'into', kind: 'text', label: 'Variable', required: true, maxLength: 64 },
      { name: 'value', kind: 'template', label: 'Value', required: true, maxLength: 4096 },
    ],
  },
  {
    kind: 'script',
    title: 'Script',
    description: 'Run a sandboxed script.',
    class: 'compute',
    phase: 'any',
    summary: null,
    produces: [],
    fields: [{ name: 'source', kind: 'code', label: 'Script', language: 'script', maxBytes: 16 * 1024 }],
  },
  {
    kind: 'increment',
    title: 'Increment counter',
    description: 'Add to a persistent counter.',
    class: 'state',
    phase: 'any',
    summary: '{counter} += {by}',
    produces: ['counter.{counter}'],
    fields: [
      { name: 'counter', kind: 'text', label: 'Counter', required: true, maxLength: 64 },
      { name: 'by', kind: 'number', label: 'Increase by', min: -1000000, max: 1000000, defaultValue: 1 },
    ],
  },
  {
    kind: 'http',
    title: 'Send request',
    description: 'Call another URL from the server.',
    class: 'fetch',
    phase: 'after',
    summary: '{method} {url}',
    produces: ['step.{@name}.status', 'step.{@name}.body'],
    fields: [
      { name: 'method', kind: 'select', label: 'Method', required: true, options: HTTP_METHODS },
      { name: 'url', kind: 'template', label: 'URL', required: true, maxLength: 2048 },
      { name: 'headers', kind: 'key_value', label: 'Request headers', valueKind: 'template', nameKind: 'header_name' },
      { name: 'body', kind: 'template', label: 'Body', nullable: true, multiline: true, maxBytes: 64 * 1024 },
      { name: 'timeout_ms', kind: 'duration', label: 'Timeout', min: 100, max: 15000, defaultValue: 10000 },
      { name: 'follow_redirects', kind: 'boolean', label: 'Follow redirects', defaultValue: true },
    ],
  },
  {
    kind: 'notify',
    title: 'Notify',
    description: 'Send a message to a notification channel.',
    class: 'effect',
    phase: 'after',
    summary: 'via {channel_id}',
    produces: [],
    fields: [
      { name: 'channel_id', kind: 'channel', label: 'Channel', required: true },
      { name: 'message', kind: 'template', label: 'Message', nullable: true, multiline: true, maxLength: 2000 },
    ],
  },
  {
    kind: 'store_s3',
    title: 'Store in S3',
    description: 'Write the capture to an object store.',
    class: 'effect',
    phase: 'after',
    summary: '{bucket_url}/{prefix}',
    produces: [],
    fields: [
      { name: 'bucket_url', kind: 'text', label: 'Bucket URL', required: true, maxLength: 512 },
      { name: 'prefix', kind: 'text', label: 'Prefix', maxLength: 256 },
      { name: 'access_key_secret', kind: 'secret', label: 'Access key secret', required: true },
      { name: 'secret_key_secret', kind: 'secret', label: 'Secret key secret', required: true },
    ],
  },
];

export const STEP_VARIABLES: readonly VariableEntry[] = [
  { path: 'request.method', type: 'string', sample: '"POST"', insertText: '{{ request.method }}', group: 'request' },
  {
    path: 'request.json.data.amount',
    type: 'number',
    sample: '2000',
    insertText: '{{ request.json.data.amount }}',
    group: 'request',
  },
  { path: 'var.amount', type: 'number', sample: '2000', insertText: '{{ var.amount }}', group: 'rules' },
  { path: 'step.crm.status', type: 'number', sample: '200', insertText: '{{ step.crm.status }}', group: 'rules' },
];

export const STEP_CONTEXT: StepEditorContext = {
  variables: STEP_VARIABLES,
  secrets: ['CRM_TOKEN', 'ARCHIVE_ACCESS', 'ARCHIVE_SIGNING'],
  channels: [
    { id: 'channel-ops', label: 'Ops chat' },
    { id: 'channel-mail', label: 'Team mailbox' },
  ],
};

export const SAMPLE_STEPS: readonly RuleStep[] = [
  {
    id: 'sample-extract',
    kind: 'extract',
    name: null,
    continueOnError: false,
    guard: null,
    fields: { from: 'request.body', json_path: '$.data.amount', regex: null, into: 'amount' },
  },
  {
    id: 'sample-respond',
    kind: 'respond',
    name: null,
    continueOnError: false,
    guard: null,
    fields: {
      status: '200',
      content_type: 'application/json',
      headers: [{ name: 'X-Amount', value: '{{ var.amount }}' }],
      body: '{"amount": {{ var.amount }}}',
    },
  },
  {
    id: 'sample-http',
    kind: 'http',
    name: 'crm',
    continueOnError: false,
    guard: { value: 'var.amount', op: 'gt', arg: 0 },
    fields: {
      method: 'POST',
      url: 'https://crm.example.com/hooks/paid',
      headers: [],
      body: null,
      timeout_ms: 10000,
      follow_redirects: true,
    },
  },
];

export const SAMPLE_ERRORS: readonly StepError[] = [
  { stepId: 'sample-http', field: 'url', message: 'The address host is not allowed on this plan.' },
  { stepId: 'sample-respond', field: null, message: 'A response step cannot come after a step that sends a request.' },
];
