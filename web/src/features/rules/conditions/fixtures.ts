import type { VariableEntry } from '../../../components/VariablePicker/variables';
import type { ConditionCatalogue, RuleConditionsSource } from './types';

export const CONDITION_CATALOGUE: ConditionCatalogue = {
  subjects: [
    {
      id: 'method',
      label: 'Method',
      prefix: 'request.method',
      kinds: ['http'],
      value_type: 'string',
      parameter: null,
      suggestions: ['GET', 'POST', 'PUT', 'PATCH', 'DELETE'],
    },
    { id: 'path', label: 'Path', prefix: 'request.path', kinds: ['http'], value_type: 'string', parameter: null },
    {
      id: 'query',
      label: 'Query parameter',
      prefix: 'request.query.',
      kinds: ['http'],
      value_type: 'string',
      parameter: { kind: 'name' },
    },
    {
      id: 'header',
      label: 'Header',
      prefix: 'request.header.',
      kinds: ['http'],
      value_type: 'string',
      parameter: { kind: 'header_name' },
    },
    {
      id: 'json',
      label: 'JSON body field',
      prefix: 'request.json.',
      kinds: ['http'],
      value_type: 'string',
      parameter: { kind: 'json_path' },
    },
    {
      id: 'amount',
      label: 'Body amount',
      prefix: 'request.json.amount',
      kinds: ['http'],
      value_type: 'number',
      parameter: null,
    },
    { id: 'body', label: 'Body text', prefix: 'request.body', kinds: ['http'], value_type: 'string', parameter: null },
    {
      id: 'sender',
      label: 'Sender address',
      prefix: 'request.client_ip',
      kinds: ['http', 'email'],
      value_type: 'ip',
      parameter: null,
    },
    {
      id: 'email_from',
      label: 'Email from',
      prefix: 'email.from',
      kinds: ['email'],
      value_type: 'string',
      parameter: null,
    },
    {
      id: 'email_subject',
      label: 'Email subject',
      prefix: 'email.subject',
      kinds: ['email'],
      value_type: 'string',
      parameter: null,
    },
    {
      id: 'dns_name',
      label: 'DNS name',
      prefix: 'dns.name',
      kinds: ['dns'],
      value_type: 'string',
      parameter: null,
    },
    {
      id: 'dns_type',
      label: 'DNS type',
      prefix: 'dns.type',
      kinds: ['dns'],
      value_type: 'string',
      parameter: null,
      suggestions: ['A', 'AAAA', 'MX', 'TXT'],
    },
    {
      id: 'vars',
      label: 'Rule variable',
      prefix: 'vars.',
      kinds: ['http', 'email', 'dns'],
      value_type: 'string',
      parameter: { kind: 'name' },
    },
  ],
  operators: [
    { id: 'eq', label: 'equals', value_types: ['string', 'number', 'ip'], arg: 'value' },
    { id: 'ne', label: 'differs from', value_types: ['string', 'number', 'ip'], arg: 'value' },
    { id: 'contains', label: 'contains', value_types: ['string'], arg: 'value' },
    { id: 'starts_with', label: 'starts with', value_types: ['string'], arg: 'value' },
    { id: 'ends_with', label: 'ends with', value_types: ['string'], arg: 'value' },
    { id: 'matches', label: 'matches', value_types: ['string'], arg: 'regex' },
    { id: 'exists', label: 'exists', value_types: ['string', 'number', 'ip'], arg: 'none' },
    { id: 'gt', label: 'greater than', value_types: ['number'], arg: 'number' },
    { id: 'lt', label: 'less than', value_types: ['number'], arg: 'number' },
    { id: 'in', label: 'in list', value_types: ['string', 'number'], arg: 'list' },
    { id: 'in_cidr', label: 'in network', value_types: ['ip'], arg: 'cidr_list' },
  ],
};

export const SAMPLE_VARIABLES: readonly VariableEntry[] = [
  { path: 'method', type: 'string', sample: '"POST"', insertText: '{{ request.method }}', group: 'request' },
  { path: 'json.id', type: 'string', sample: '"evt_demo_1"', insertText: '{{ request.json.id }}', group: 'request' },
  {
    path: 'header.x-event-type',
    type: 'string',
    sample: '"refund.created"',
    insertText: '{{ request.header.x-event-type }}',
    group: 'request',
  },
  { path: 'vars.amount', description: 'set by step 1', insertText: '{{ vars.amount }}', group: 'rules' },
];

export const SAMPLE_VERSION = 7;

export function sampleSource(overrides: Partial<RuleConditionsSource> = {}): RuleConditionsSource {
  return {
    version: SAMPLE_VERSION,
    triggers: ['http'],
    on_match: 'next',
    condition: {
      all: [
        { value: 'request.method', op: 'eq', arg: 'POST' },
        { value: 'request.header.x-event-type', op: 'eq', arg: 'refund.created' },
        {
          any: [
            { value: 'request.json.status', op: 'in', arg: ['paid', 'settled'] },
            { not: { value: 'request.client_ip', op: 'in_cidr', arg: ['203.0.113.0/24'] } },
          ],
        },
      ],
    },
    ...overrides,
  };
}

export const UNSUPPORTED_CONDITION: unknown = { all: [{ value: 'request.method', op: 'eq', arg: 'GET', extra: true }] };
