import { describe, expect, it } from 'vitest';
import { openApiFixture } from '../../test/fixtures/openapi';
import { parseOpenApiDocument } from './document';
import { buildReference } from './reference';

const document = parseOpenApiDocument(openApiFixture);
const reference = buildReference(document);

function operationById(id: string) {
  const found = reference.operations.find((operation) => operation.id === id);
  if (found === undefined) {
    throw new Error(`The fixture has no operation ${id}`);
  }
  return found;
}

describe('buildReference', () => {
  it('lists every tag and every operation of the document once', () => {
    const operationCount = Object.values(document.paths).reduce<number>(
      (total, item) => total + Object.keys(item as object).length,
      0,
    );
    const tagCount = (document.tags as unknown[]).length;

    expect(reference.operations).toHaveLength(operationCount);
    expect(reference.tags).toHaveLength(tagCount);
    expect(reference.tags.reduce((total, tag) => total + tag.operations.length, 0)).toBe(operationCount);
    expect(new Set(reference.operations.map((operation) => operation.id)).size).toBe(operationCount);
    expect(reference.version).toBe('0.1.0-dev');
    expect(reference.serverUrl).toBe('/api/v1');
  });

  it('turns security requirements into chips, with an empty requirement meaning public', () => {
    expect(operationById('tokens_create').security.map((chip) => chip.kind)).toEqual(['session']);
    expect(operationById('endpoints_get').security.map((chip) => chip.kind)).toEqual(['session', 'token', 'public']);
    expect(operationById('openapi_get').security.map((chip) => chip.kind)).toEqual(['public']);
    expect(operationById('endpoints_get').acceptsToken).toBe(true);
    expect(operationById('tokens_create').acceptsToken).toBe(false);
  });

  it('reads parameters, the request body and the responses in status order, resolving shared responses', () => {
    const operation = operationById('captures_list');
    const id = operation.parameters.find((parameter) => parameter.name === 'id');
    expect(id).toMatchObject({ location: 'path', required: true });
    expect(operation.parameters.some((parameter) => parameter.location === 'header')).toBe(true);

    const statuses = operation.responses.map((response) => response.status);
    expect(statuses).toEqual([...statuses].sort((left, right) => Number(left) - Number(right)));
    const limited = operation.responses.find((response) => response.status === '429');
    expect(limited?.description).toContain('rate_limited');
    expect(limited?.headers.map((header) => header.name)).toContain('Retry-After');

    expect(operationById('tokens_create').requestBody?.content[0]?.mediaType).toBe('application/json');
  });

  it('builds ids for operations without one, merges path level parameters and applies document security', () => {
    const built = buildReference(
      parseOpenApiDocument({
        openapi: '3.1.0',
        security: [{ api_token: [] }],
        components: { securitySchemes: { api_token: { type: 'http', scheme: 'bearer' } } },
        paths: {
          '/items/{id}': {
            parameters: [{ name: 'id', in: 'path', required: true, schema: { type: 'string' } }],
            get: { summary: 'Read an item', responses: {} },
            delete: { operationId: 'items_delete', security: [], responses: {} },
          },
        },
      }),
    );

    expect(built.operations.map((operation) => operation.id)).toEqual(['get_items_id', 'items_delete']);
    expect(built.operations[0]?.parameters.map((parameter) => parameter.name)).toEqual(['id']);
    expect(built.operations[0]?.security.map((chip) => chip.kind)).toEqual(['token']);
    expect(built.operations[1]?.security.map((chip) => chip.kind)).toEqual(['public']);
    expect(built.tags).toHaveLength(1);
    expect(built.tags[0]?.name).toBeNull();
  });
});
