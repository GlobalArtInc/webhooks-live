import { describe, expect, it } from 'vitest';
import { openApiFixture } from '../../test/fixtures/openapi';
import { parseOpenApiDocument } from './document';
import { describeSchema } from './schemaTree';
import { miniDocument, schemaRef } from './testDocuments';

function names(nodes: readonly { name: string | null }[]): (string | null)[] {
  return nodes.map((node) => node.name);
}

describe('describeSchema', () => {
  it('resolves $ref and keeps the component name and the required marks', () => {
    const document = miniDocument({
      Page: {
        type: 'object',
        required: ['items'],
        properties: { items: { type: 'array', items: schemaRef('Item') }, cursor: { type: 'string' } },
      },
      Item: { type: 'object', properties: { id: { type: 'string', pattern: '^a+$', maxLength: 5 } } },
    });
    const page = describeSchema(document, schemaRef('Page'));

    expect(page.refName).toBe('Page');
    expect(page.properties().map((property) => [property.name, property.required])).toEqual([
      ['items', true],
      ['cursor', false],
    ]);
    const items = page.properties()[0];
    expect(items?.type).toBe('array');
    expect(items?.items?.refName).toBe('Item');
    expect(items?.items?.properties()[0]?.constraints).toEqual([
      { kind: 'maxLength', value: '5' },
      { kind: 'pattern', value: '^a+$' },
    ]);
  });

  it('merges allOf members into one object', () => {
    const document = parseOpenApiDocument(openApiFixture);
    const capture = describeSchema(document, schemaRef('Capture'));
    const summary = describeSchema(document, schemaRef('CaptureSummary'));

    const merged = names(capture.properties());
    for (const name of names(summary.properties())) {
      expect(merged).toContain(name);
    }
    expect(merged).toContain('headers');
    expect(capture.type).toBe('object');
  });

  it('reads 3.1 nullable unions both as a type list and as oneOf with null', () => {
    const document = miniDocument({
      Holder: {
        type: 'object',
        properties: {
          title: { type: ['string', 'null'], maxLength: 10 },
          verifier: { oneOf: [{ type: 'null' }, schemaRef('Verifier')] },
        },
      },
      Verifier: { type: 'object', properties: { kind: { type: 'string', enum: ['hmac'] } } },
    });
    const [title, verifier] = describeSchema(document, schemaRef('Holder')).properties();

    expect(title).toMatchObject({ type: 'string', nullable: true });
    expect(verifier).toMatchObject({ refName: 'Verifier', type: 'object', nullable: true, alternatives: [] });
    expect(verifier?.properties()[0]?.enumValues).toEqual(['"hmac"']);
  });

  it('shows oneOf with several members as alternatives', () => {
    const document = parseOpenApiDocument(openApiFixture);
    const auth = describeSchema(document, schemaRef('CaptureAuth'));

    expect(auth.alternatives).toHaveLength(2);
    expect(auth.alternatives.map((alternative) => names(alternative.properties()))).toEqual([
      ['kind', 'username'],
      ['kind', 'name'],
    ]);
    expect(auth.expandable).toBe(true);
  });

  it('marks a cycle as recursive instead of expanding it forever', () => {
    const document = miniDocument({
      Node: {
        type: 'object',
        properties: { next: schemaRef('Node'), children: { type: 'array', items: schemaRef('Node') } },
      },
    });
    const node = describeSchema(document, schemaRef('Node'));
    const [next, children] = node.properties();

    expect(next).toMatchObject({ recursive: true, refName: 'Node', expandable: false });
    expect(children?.items).toMatchObject({ recursive: true });
    expect(children?.expandable).toBe(true);
  });

  it('stops at the depth limit and says so', () => {
    const deep: Record<string, unknown> = { type: 'object', properties: {} };
    let cursor = deep;
    for (let level = 0; level < 12; level += 1) {
      const next: Record<string, unknown> = { type: 'object', properties: {} };
      (cursor.properties as Record<string, unknown>).child = next;
      cursor = next;
    }
    (cursor.properties as Record<string, unknown>).leaf = { type: 'string' };
    let current = describeSchema(miniDocument({}), deep);
    let depth = 0;
    while (current.expandable) {
      current = current.properties()[0] ?? current;
      depth += 1;
    }

    expect(current.truncated).toBe(true);
    expect(depth).toBeLessThan(12);
  });

  it('describes a bare object and an untyped schema without inventing structure', () => {
    const document = miniDocument({});

    expect(describeSchema(document, { type: 'object' })).toMatchObject({ type: 'object', expandable: false });
    expect(describeSchema(document, {})).toMatchObject({ type: null, expandable: false });
    expect(describeSchema(document, { $ref: '#/components/schemas/Missing' })).toMatchObject({
      refName: 'Missing',
      type: null,
    });
  });
});
