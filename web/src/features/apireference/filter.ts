import type { Reference, ReferenceOperation } from './reference';

export function filterTerms(query: string): string[] {
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter((term) => term !== '');
}

export function operationMatches(operation: ReferenceOperation, terms: readonly string[]): boolean {
  return terms.every((term) => operation.searchText.includes(term));
}

export function filterReference(reference: Reference, query: string): Reference {
  const terms = filterTerms(query);
  if (terms.length === 0) {
    return reference;
  }
  const tags = reference.tags
    .map((tag) => ({ ...tag, operations: tag.operations.filter((operation) => operationMatches(operation, terms)) }))
    .filter((tag) => tag.operations.length > 0);
  return { ...reference, tags, operations: tags.flatMap((tag) => tag.operations) };
}
