import type { OpenApiDocument } from './document';
import { arrayOf, isObject, objectOf, stringOf, stringsOf, without, type JsonObject } from './json';

export const MAX_SCHEMA_DEPTH = 8;
const MAX_REF_HOPS = 32;
const MAX_NORMALIZE_PASSES = 8;
const LOCAL_REF_PREFIX = '#/';

export interface PointerTarget {
  name: string;
  value: unknown;
}

export interface NormalizedSchema {
  schema: JsonObject;
  refName: string | null;
  nullable: boolean;
  alternatives: unknown[];
  recursive: boolean;
  trail: readonly string[];
}

function decodePointerSegment(segment: string): string {
  let decoded = segment;
  try {
    decoded = decodeURIComponent(segment);
  } catch {
    decoded = segment;
  }
  return decoded.replaceAll('~1', '/').replaceAll('~0', '~');
}

export function resolvePointer(document: OpenApiDocument, ref: string): PointerTarget | null {
  if (!ref.startsWith(LOCAL_REF_PREFIX)) {
    return null;
  }
  const segments = ref.slice(LOCAL_REF_PREFIX.length).split('/').map(decodePointerSegment);
  let current: unknown = document;
  for (const segment of segments) {
    if (!isObject(current) && !Array.isArray(current)) {
      return null;
    }
    current = (current as Record<string, unknown>)[segment];
    if (current === undefined) {
      return null;
    }
  }
  return { name: segments[segments.length - 1] ?? ref, value: current };
}

export function resolveObjectRef(document: OpenApiDocument, value: unknown): JsonObject {
  let current = objectOf(value);
  for (let hops = 0; hops < MAX_REF_HOPS; hops += 1) {
    const ref = stringOf(current.$ref);
    if (ref === null) {
      return current;
    }
    const target = resolvePointer(document, ref);
    if (target === null) {
      return without(current, '$ref');
    }
    current = { ...objectOf(target.value), ...without(current, '$ref') };
  }
  return {};
}

function isNullSchema(schema: JsonObject): boolean {
  return schema.type === 'null' || schema.const === null;
}

export function mergeSchemas(first: JsonObject, second: JsonObject): JsonObject {
  const merged: JsonObject = { ...second, ...first };
  const properties = { ...objectOf(second.properties), ...objectOf(first.properties) };
  if (Object.keys(properties).length > 0) {
    merged.properties = properties;
  }
  const required = [...new Set([...stringsOf(first.required), ...stringsOf(second.required)])];
  if (required.length > 0) {
    merged.required = required;
  }
  return merged;
}

interface FlattenResult {
  schema: JsonObject;
  recursive: boolean;
}

function resolveRef(
  document: OpenApiDocument,
  raw: unknown,
  trail: readonly string[],
): { schema: JsonObject; refName: string | null; recursive: boolean; trail: readonly string[] } {
  let current = objectOf(raw);
  let refName: string | null = null;
  let path = trail;
  for (let hops = 0; hops < MAX_REF_HOPS; hops += 1) {
    const ref = stringOf(current.$ref);
    if (ref === null) {
      return { schema: current, refName, recursive: false, trail: path };
    }
    const target = resolvePointer(document, ref);
    const name = target?.name ?? ref.split('/').pop() ?? ref;
    refName ??= name;
    if (target === null) {
      return { schema: without(current, '$ref'), refName, recursive: false, trail: path };
    }
    if (path.includes(ref)) {
      return { schema: {}, refName, recursive: true, trail: path };
    }
    path = [...path, ref];
    current = { ...objectOf(target.value), ...without(current, '$ref') };
  }
  return { schema: {}, refName, recursive: true, trail: path };
}

function flattenAllOf(document: OpenApiDocument, schema: JsonObject, trail: readonly string[]): FlattenResult {
  const members = arrayOf(schema.allOf);
  if (members.length === 0) {
    return { schema, recursive: false };
  }
  let merged = without(schema, 'allOf');
  let recursive = false;
  for (const member of members) {
    const resolved = resolveRef(document, member, trail);
    if (resolved.recursive) {
      recursive = true;
      continue;
    }
    const inner = flattenAllOf(document, resolved.schema, resolved.trail);
    recursive = recursive || inner.recursive;
    merged = mergeSchemas(merged, inner.schema);
  }
  return { schema: merged, recursive };
}

export function normalizeSchema(document: OpenApiDocument, raw: unknown, trail: readonly string[]): NormalizedSchema {
  const first = resolveRef(document, raw, trail);
  if (first.recursive) {
    return {
      schema: {},
      refName: first.refName,
      nullable: false,
      alternatives: [],
      recursive: true,
      trail: first.trail,
    };
  }
  let schema = first.schema;
  let refName = first.refName;
  let path = first.trail;
  let nullable = false;
  let recursive = false;
  let alternatives: unknown[] = [];
  for (let pass = 0; pass < MAX_NORMALIZE_PASSES; pass += 1) {
    const flat = flattenAllOf(document, schema, path);
    schema = flat.schema;
    recursive = recursive || flat.recursive;
    const options = arrayOf(schema.oneOf ?? schema.anyOf);
    if (options.length === 0) {
      alternatives = [];
      break;
    }
    const resolved = options.map((option) => ({ option, ...resolveRef(document, option, path) }));
    const present = resolved.filter((entry) => !isNullSchema(entry.schema));
    nullable = nullable || present.length < resolved.length;
    const [only] = present;
    if (present.length === 1 && only !== undefined) {
      if (only.recursive) {
        recursive = true;
        alternatives = [];
        schema = without(schema, 'oneOf', 'anyOf');
        break;
      }
      schema = mergeSchemas(without(schema, 'oneOf', 'anyOf'), only.schema);
      refName ??= only.refName;
      path = only.trail;
      continue;
    }
    alternatives = present.map((entry) => entry.option);
    break;
  }
  const types = typeNames(schema);
  if (types.includes('null')) {
    nullable = true;
  }
  return { schema, refName, nullable, alternatives, recursive, trail: path };
}

export function typeNames(schema: JsonObject): string[] {
  return typeof schema.type === 'string' ? [schema.type] : stringsOf(schema.type);
}

export function primaryType(schema: JsonObject): string | null {
  const names = typeNames(schema).filter((name) => name !== 'null');
  if (names.length > 0) {
    return names.join(' | ');
  }
  if (typeNames(schema).includes('null')) {
    return 'null';
  }
  if (isObject(schema.properties)) {
    return 'object';
  }
  if (schema.items !== undefined) {
    return 'array';
  }
  return null;
}
