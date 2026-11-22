import { describe, expect, it } from 'vitest';
import { openApiFixture } from '../../test/fixtures/openapi';
import type { ResponseSettingsDraft } from '../workspace/response/responseSettings';
import { parseCatalogue, parsePreview } from './api';
import { previewRequestOf } from './previewRequest';

const PREVIEW_PATH = '/endpoints/{id}/template/preview';
const VARIABLES_PATH = '/endpoints/{id}/template/variables';

type Json = Record<string, unknown>;

const PREVIEW_ANSWER: Json = {
  capture_id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21',
  rendered: {
    status: 201,
    content_type: 'application/json',
    headers: [{ name: 'X-Mode', value: 'test' }],
    body: { encoding: 'utf8', data: '{"id":"evt_1"}', truncated: false },
    body_size: 14,
    duration_ms: 0,
  },
  diagnostics: [
    {
      field: 'response.body',
      code: 'value_missing',
      severity: 'warning',
      line: 1,
      column: 6,
      length: 22,
      message: 'request.query.missing has no value and renders empty',
    },
  ],
};

const CATALOGUE_ANSWER: Json = {
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
      path: 'json("$.data.amount")',
      insert_text: '{{ json("$.data.amount") }}',
      group: 'request',
      type: 'integer',
      sample: '2000',
      description: null,
    },
  ],
};

const DRAFT: ResponseSettingsDraft = {
  status: '200',
  contentType: 'application/json',
  body: '{}',
  bodyEncoding: 'utf8',
  delay: '0',
  cors: false,
  overflow: 'refuse',
  templated: true,
  headers: [{ key: 1, name: 'x-order', value: '{{ request.path }}' }],
};

function isJson(value: unknown): value is Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function field(value: unknown, key: string): unknown {
  return isJson(value) ? value[key] : undefined;
}

function schemas(): Json {
  const found = field(field(openApiFixture, 'components'), 'schemas');
  if (!isJson(found)) {
    throw new Error('the document has no schemas');
  }
  return found;
}

function propertiesOf(name: string): string[] {
  const properties = field(schemas()[name], 'properties');
  return isJson(properties) ? Object.keys(properties) : [];
}

describe('the template routes the server documents', () => {
  it('serves the preview by POST and the catalogue by GET with a capture_id query', () => {
    const paths = field(openApiFixture, 'paths');
    expect(isJson(field(field(paths, PREVIEW_PATH), 'post'))).toBe(true);
    const variables = field(field(paths, VARIABLES_PATH), 'get');
    const parameters = field(variables, 'parameters');
    const names = Array.isArray(parameters) ? parameters.map((item) => field(item, 'name')) : [];
    expect(names).toContain('capture_id');
  });

  it('accepts the request the interface builds', () => {
    const request = previewRequestOf(DRAFT, 'c1');

    expect(Object.keys(request).every((key) => propertiesOf('TemplatePreviewInput').includes(key))).toBe(true);
    expect(
      Object.keys(request.response).every((key) => propertiesOf('TemplatePreviewResponseInput').includes(key)),
    ).toBe(true);
  });

  it('answers with the fields the interface reads', () => {
    const rendered = field(PREVIEW_ANSWER, 'rendered');
    expect(Object.keys(PREVIEW_ANSWER).sort()).toEqual(propertiesOf('TemplatePreview').sort());
    expect(Object.keys(isJson(rendered) ? rendered : {}).sort()).toEqual(propertiesOf('RenderedResponse').sort());
    const diagnostics = field(PREVIEW_ANSWER, 'diagnostics');
    const first = Array.isArray(diagnostics) ? diagnostics[0] : undefined;
    expect(Object.keys(isJson(first) ? first : {}).sort()).toEqual(propertiesOf('PreviewDiagnostic').sort());

    const result = parsePreview(PREVIEW_ANSWER);

    expect(result.rendered?.status).toBe(201);
    expect(result.rendered?.body.data).toBe('{"id":"evt_1"}');
    expect(result.diagnostics[0]).toMatchObject({ field: 'response.body', severity: 'warning', length: 22 });
  });

  it('lists the catalogue with the fields the picker reads', () => {
    const items = field(CATALOGUE_ANSWER, 'items');
    const first = Array.isArray(items) ? items[0] : undefined;
    expect(Object.keys(CATALOGUE_ANSWER).sort()).toEqual(propertiesOf('VariableCatalogue').sort());
    expect(Object.keys(isJson(first) ? first : {}).sort()).toEqual(propertiesOf('CatalogueVariable').sort());

    const catalogue = parseCatalogue(CATALOGUE_ANSWER);

    expect(catalogue.items.map((item) => item.path)).toEqual(['request.method', 'json("$.data.amount")']);
    expect(catalogue.items[1]).toMatchObject({ type: 'integer', sample: '2000', description: null });
  });
});
