import type { Endpoint } from '../../api/types';
import type { VariableCatalogue } from '../../features/templating/types';
import { endpointFixture } from './endpoint';

export const VALID_TEMPLATE_BODY = '{"received":"{{ request.json.id }}","path":"{{ request.path }}"}';
export const INVALID_TEMPLATE_BODY = '{"received":"{{ request.json.id }}","at":"{{ now | isodate }}"}';
export const WARNING_TEMPLATE_BODY = '{"mode":"{{ request.query.missing }}"}';

export const catalogueFixture: VariableCatalogue = {
  delimiters: { open: '{{', close: '}}' },
  items: [
    {
      path: 'method',
      insert_text: '{{ request.method }}',
      group: 'request',
      type: 'string',
      sample: '"POST"',
      description: null,
    },
    {
      path: 'path',
      insert_text: '{{ request.path }}',
      group: 'request',
      type: 'string',
      sample: '"/hook/2"',
      description: null,
    },
    {
      path: 'json.id',
      insert_text: '{{ request.json.id }}',
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
      description: 'The time of the capture',
    },
    {
      path: 'uuid',
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
