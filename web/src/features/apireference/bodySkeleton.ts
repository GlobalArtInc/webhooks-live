import type { OpenApiDocument } from './document';
import { arrayOf, isObject, numberOf, objectOf, stringsOf } from './json';
import { normalizeSchema, primaryType } from './schemaResolve';

export const MAX_SKELETON_DEPTH = 5;
const MAX_OPTIONAL_WITH_EXAMPLE = 5;

const STRING_PLACEHOLDERS: Readonly<Record<string, string>> = {
  'date-time': '2026-01-01T00:00:00Z',
  date: '2026-01-01',
  time: '00:00:00Z',
  email: 'user@example.com',
  uri: 'https://example.com',
  url: 'https://example.com',
  uuid: '00000000-0000-4000-8000-000000000000',
  hostname: 'example.com',
  ipv4: '192.0.2.1',
  ipv6: '2001:db8::1',
};

function exampleIn(schema: Record<string, unknown>): { found: boolean; value: unknown } {
  if ('example' in schema) {
    return { found: true, value: schema.example };
  }
  const [first] = arrayOf(schema.examples);
  return first === undefined ? { found: false, value: undefined } : { found: true, value: first };
}

function stringPlaceholder(format: unknown): string {
  return typeof format === 'string' ? (STRING_PLACEHOLDERS[format] ?? 'string') : 'string';
}

function numberPlaceholder(schema: Record<string, unknown>): number {
  const minimum = numberOf(schema.minimum);
  if (minimum !== null) {
    return minimum;
  }
  const maximum = numberOf(schema.maximum);
  return maximum !== null && maximum < 0 ? maximum : 0;
}

function objectSkeleton(
  document: OpenApiDocument,
  schema: Record<string, unknown>,
  trail: readonly string[],
  depth: number,
): Record<string, unknown> {
  const properties = objectOf(schema.properties);
  const required = new Set(stringsOf(schema.required));
  const result: Record<string, unknown> = {};
  const requiredNames = Object.keys(properties).filter((key) => required.has(key));
  const chosen =
    requiredNames.length > 0
      ? requiredNames
      : Object.keys(properties)
          .filter((key) => exampleIn(normalizeSchema(document, properties[key], trail).schema).found)
          .slice(0, MAX_OPTIONAL_WITH_EXAMPLE);
  for (const key of chosen) {
    const value = skeletonValue(document, properties[key], trail, depth + 1);
    if (value !== undefined) {
      result[key] = value;
    }
  }
  if (Object.keys(properties).length === 0 && isObject(schema.additionalProperties)) {
    const value = skeletonValue(document, schema.additionalProperties, trail, depth + 1);
    if (value !== undefined) {
      result.key = value;
    }
  }
  return result;
}

function skeletonValue(document: OpenApiDocument, raw: unknown, trail: readonly string[], depth: number): unknown {
  const normalized = normalizeSchema(document, raw, trail);
  if (normalized.recursive) {
    return undefined;
  }
  const { schema } = normalized;
  if (schema.readOnly === true) {
    return undefined;
  }
  const example = exampleIn(schema);
  if (example.found) {
    return example.value;
  }
  if ('const' in schema) {
    return schema.const;
  }
  if ('default' in schema) {
    return schema.default;
  }
  const [firstEnum] = arrayOf(schema.enum).filter((value) => value !== null);
  if (firstEnum !== undefined) {
    return firstEnum;
  }
  const [firstAlternative] = normalized.alternatives;
  if (firstAlternative !== undefined) {
    return skeletonValue(document, firstAlternative, normalized.trail, depth);
  }
  const type = primaryType(schema)?.split(' | ')[0] ?? null;
  switch (type) {
    case 'object':
      return depth >= MAX_SKELETON_DEPTH ? {} : objectSkeleton(document, schema, normalized.trail, depth);
    case 'array': {
      if (depth >= MAX_SKELETON_DEPTH) {
        return [];
      }
      const item = skeletonValue(document, schema.items, normalized.trail, depth + 1);
      return item === undefined ? [] : [item];
    }
    case 'string':
      return stringPlaceholder(schema.format);
    case 'integer':
    case 'number':
      return numberPlaceholder(schema);
    case 'boolean':
      return false;
    case 'null':
      return null;
    default:
      return null;
  }
}

export function buildBodySkeleton(document: OpenApiDocument, schema: unknown): unknown {
  const value = skeletonValue(document, schema, [], 0);
  return value === undefined ? {} : value;
}
