import { describe, expect, it } from 'vitest';
import { openApiFixture } from '../../test/fixtures/openapi';
import { buildBodySkeleton } from './bodySkeleton';
import { parseOpenApiDocument } from './document';
import { buildReference } from './reference';
import { buildSample, formatCurl, formatHttp, type SampleContext } from './samples';
import { miniDocument, schemaRef } from './testDocuments';

const document = parseOpenApiDocument(openApiFixture);
const reference = buildReference(document);
const context: SampleContext = { origin: 'https://hooks.example', serverUrl: '/api/v1' };

function sampleOf(id: string) {
  const operation = reference.operations.find((candidate) => candidate.id === id);
  if (operation === undefined) {
    throw new Error(`The fixture has no operation ${id}`);
  }
  return buildSample(operation, context);
}

describe('samples', () => {
  it('fills a path parameter from its example and sends the token variable where a token is accepted', () => {
    const curl = formatCurl(sampleOf('endpoints_get'));

    expect(curl).toBe(
      [
        'curl --request GET \\',
        "  --url 'https://hooks.example/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4' \\",
        '  --header "Authorization: Bearer $WEBHOOKS_LIVE_TOKEN"',
      ].join('\n'),
    );
    expect(formatHttp(sampleOf('endpoints_get'))).toContain('Authorization: Bearer <token>');
  });

  it('builds a JSON body from the required fields and their examples', () => {
    const sample = sampleOf('tokens_create');
    const curl = formatCurl(sample);

    expect(curl).toContain("--header 'Content-Type: application/json'");
    expect(curl).not.toContain('Authorization');
    expect(JSON.parse(sample.body ?? '')).toEqual({ name: 'laptop cli', scopes: ['read'] });
    expect(formatCurl(sampleOf('endpoints_create'))).toContain("--data '{}'");
  });

  it('sends nothing but the URL for a public operation', () => {
    const sample = sampleOf('openapi_get');

    expect(sample.headers).toEqual([]);
    expect(sample.body).toBeNull();
    expect(formatHttp(sample)).toBe('GET /api/v1/openapi.json HTTP/1.1\nHost: hooks.example');
  });

  it('uses placeholders, required query parameters, absolute servers and shell quoting', () => {
    const generated = buildReference(
      parseOpenApiDocument({
        openapi: '3.1.0',
        servers: [{ url: 'https://{region}.example.com/v2/', variables: { region: { default: 'eu' } } }],
        paths: {
          '/jobs/{job}': {
            post: {
              operationId: 'jobs_run',
              parameters: [
                { name: 'job', in: 'path', required: true, schema: { type: 'string' } },
                { name: 'mode', in: 'query', required: true, schema: { type: 'string', enum: ['fast', 'slow'] } },
              ],
              requestBody: {
                content: { 'application/json': { schema: { type: 'object', example: { note: "it's" } } } },
              },
              responses: {},
            },
          },
        },
      }),
    );
    const [operation] = generated.operations;
    if (operation === undefined) {
      throw new Error('No operation');
    }
    const curl = formatCurl(buildSample(operation, { origin: 'https://app.example', serverUrl: generated.serverUrl }));

    expect(curl).toContain("--url 'https://eu.example.com/v2/jobs/<job>?mode=fast'");
    expect(curl).toContain(`--data '{\n  "note": "it'\\''s"\n}'`);
  });
});

describe('buildBodySkeleton', () => {
  it('prefers examples, omits read-only fields, skips optional fields and survives cycles', () => {
    const doc = miniDocument({
      Input: {
        type: 'object',
        required: ['name', 'when', 'parent', 'tags', 'kind'],
        properties: {
          name: { type: 'string', example: 'Ping' },
          when: { type: 'string', format: 'date-time' },
          id: { type: 'string', readOnly: true },
          note: { type: 'string' },
          parent: schemaRef('Input'),
          tags: { type: 'array', items: { type: 'integer', minimum: 3 } },
          kind: { oneOf: [{ type: 'null' }, { type: 'string', enum: ['a', 'b'] }] },
        },
      },
    });

    expect(buildBodySkeleton(doc, schemaRef('Input'))).toEqual({
      name: 'Ping',
      when: '2026-01-01T00:00:00Z',
      tags: [3],
      kind: 'a',
    });
  });
});
