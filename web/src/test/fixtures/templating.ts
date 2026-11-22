import type { Endpoint } from '../../api/types';
import type { VariableCatalogue } from '../../features/templating/types';
import { endpointFixture } from './endpoint';

export const VALID_TEMPLATE_BODY = '{"received":"{{ json("$.id") }}","path":"{{ request.path }}"}';
export const INVALID_TEMPLATE_BODY = '{"received":"{{ json("$.id") }}","at":"{{ now | isodate }}"}';
export const WARNING_TEMPLATE_BODY = '{"mode":"{{ request.query.missing }}"}';

export const catalogueFixture: VariableCatalogue = {
  delimiters: { open: '{{', close: '}}' },
  items: [
    {
      path: 'request.method',
      insert_text: '{{ request.method }}',
      group: 'request',
      type: 'string',
      sample: '"POST"',
      description: 'The HTTP method',
    },
    {
      path: 'request.path',
      insert_text: '{{ request.path }}',
      group: 'request',
      type: 'string',
      sample: '"/hook/2"',
      description: 'The request path',
    },
    {
      path: 'request.query.mode',
      insert_text: '{{ request.query.mode }}',
      group: 'request',
      type: 'string',
      sample: '"test"',
      description: null,
    },
    {
      path: 'json("$.id")',
      insert_text: '{{ json("$.id") }}',
      group: 'request',
      type: 'string',
      sample: '"evt_2"',
      description: null,
    },
    {
      path: 'now',
      insert_text: '{{ now }}',
      group: 'time',
      type: 'time',
      sample: '2026-11-21T14:02:11Z',
      description: 'The time the capture was received',
    },
    {
      path: 'counter.hits',
      insert_text: '{{ counter.hits }}',
      group: 'variables',
      type: 'integer',
      sample: '3',
      description: 'A persistent counter of the endpoint',
    },
    {
      path: 'uuid()',
      insert_text: '{{ uuid() }}',
      group: 'helpers',
      type: 'function',
      sample: null,
      description: 'A new random UUID',
    },
  ],
};

export function templatedEndpoint(body: string): Endpoint {
  return {
    ...endpointFixture,
    response: {
      ...endpointFixture.response,
      content_type: 'application/json',
      body: { encoding: 'utf8', data: body, truncated: false },
      templated: true,
    },
  };
}
