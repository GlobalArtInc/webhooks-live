import { describe, expect, it } from 'vitest';
import { openApiFixture } from '../../test/fixtures/openapi';
import { parseOpenApiDocument } from './document';
import { filterReference } from './filter';
import { buildReference } from './reference';
import { validateApiReferenceSearch } from './search';

const reference = buildReference(parseOpenApiDocument(openApiFixture));

describe('filterReference', () => {
  it('returns the reference untouched for an empty query', () => {
    expect(filterReference(reference, '   ')).toBe(reference);
  });

  it('matches the path, the operation id and the summary, all terms together', () => {
    const byPath = filterReference(reference, '/captures/{capture_id}/forward');
    expect(byPath.operations.length).toBeGreaterThan(0);
    expect(byPath.operations.every((operation) => operation.path.includes('/captures/{capture_id}/forward'))).toBe(
      true,
    );

    const byId = filterReference(reference, 'ALIASES_');
    expect(byId.operations.map((operation) => operation.id).every((id) => id.startsWith('aliases_'))).toBe(true);
    expect(byId.tags.map((tag) => tag.name)).toEqual(['Aliases']);

    const summary = reference.operations.find((operation) => operation.id === 'aliases_availability')?.summary ?? '';
    expect(filterReference(reference, summary.toUpperCase()).operations.map((operation) => operation.id)).toContain(
      'aliases_availability',
    );

    const narrowed = filterReference(reference, 'get aliases');
    expect(
      narrowed.operations.every((operation) => operation.method === 'GET' && operation.id.startsWith('aliases_')),
    ).toBe(true);
  });

  it('drops tags with no match and finds nothing for nonsense', () => {
    const none = filterReference(reference, 'zzzz-no-such-operation');

    expect(none.tags).toEqual([]);
    expect(none.operations).toEqual([]);
  });
});

describe('validateApiReferenceSearch', () => {
  it('keeps a trimmed bounded q and drops anything else', () => {
    expect(validateApiReferenceSearch({ q: '  rules ' })).toEqual({ q: 'rules' });
    expect(validateApiReferenceSearch({ q: '   ' })).toEqual({});
    expect(validateApiReferenceSearch({ q: 7 })).toEqual({});
    expect(validateApiReferenceSearch({ q: 'x'.repeat(500) }).q).toHaveLength(200);
  });
});
