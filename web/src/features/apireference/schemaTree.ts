import type { OpenApiDocument } from './document';
import { arrayOf, formatValue, isObject, objectOf, stringOf, stringsOf, type JsonObject } from './json';
import { MAX_SCHEMA_DEPTH, normalizeSchema, primaryType } from './schemaResolve';

export type SchemaConstraintKind =
  | 'minimum'
  | 'maximum'
  | 'exclusiveMinimum'
  | 'exclusiveMaximum'
  | 'multipleOf'
  | 'minLength'
  | 'maxLength'
  | 'pattern'
  | 'minItems'
  | 'maxItems'
  | 'uniqueItems'
  | 'minProperties'
  | 'maxProperties';

export interface SchemaConstraint {
  kind: SchemaConstraintKind;
  value: string;
}

export interface SchemaNode {
  name: string | null;
  required: boolean;
  refName: string | null;
  type: string | null;
  format: string | null;
  nullable: boolean;
  description: string | null;
  enumValues: string[];
  defaultValue: string | null;
  example: string | null;
  constraints: SchemaConstraint[];
  readOnly: boolean;
  writeOnly: boolean;
  deprecated: boolean;
  items: SchemaNode | null;
  alternatives: SchemaNode[];
  additional: SchemaNode | null;
  hasProperties: boolean;
  expandable: boolean;
  recursive: boolean;
  truncated: boolean;
  properties: () => SchemaNode[];
}

const CONSTRAINT_KINDS: readonly SchemaConstraintKind[] = [
  'minimum',
  'maximum',
  'exclusiveMinimum',
  'exclusiveMaximum',
  'multipleOf',
  'minLength',
  'maxLength',
  'pattern',
  'minItems',
  'maxItems',
  'minProperties',
  'maxProperties',
];

function constraintsOf(schema: JsonObject): SchemaConstraint[] {
  const found: SchemaConstraint[] = [];
  for (const kind of CONSTRAINT_KINDS) {
    const value = schema[kind];
    if (typeof value === 'string' || typeof value === 'number') {
      found.push({ kind, value: String(value) });
    }
  }
  if (schema.uniqueItems === true) {
    found.push({ kind: 'uniqueItems', value: 'true' });
  }
  return found;
}

function exampleOf(schema: JsonObject): string | null {
  if ('example' in schema) {
    return formatValue(schema.example);
  }
  const [first] = arrayOf(schema.examples);
  return first === undefined ? null : formatValue(first);
}

function enumOf(schema: JsonObject): string[] {
  const values = arrayOf(schema.enum).map((value) => formatValue(value, 80));
  if ('const' in schema && schema.const !== null) {
    return [formatValue(schema.const, 80)];
  }
  return values.filter((value) => value !== 'null');
}

function emptyMemo(): () => SchemaNode[] {
  return () => [];
}

function memoize(build: () => SchemaNode[]): () => SchemaNode[] {
  let cached: SchemaNode[] | null = null;
  return () => {
    cached ??= build();
    return cached;
  };
}

function trailNode(name: string | null, required: boolean, refName: string | null, kind: 'recursive' | 'truncated') {
  const node: SchemaNode = {
    name,
    required,
    refName,
    type: null,
    format: null,
    nullable: false,
    description: null,
    enumValues: [],
    defaultValue: null,
    example: null,
    constraints: [],
    readOnly: false,
    writeOnly: false,
    deprecated: false,
    items: null,
    alternatives: [],
    additional: null,
    hasProperties: false,
    expandable: false,
    recursive: kind === 'recursive',
    truncated: kind === 'truncated',
    properties: emptyMemo(),
  };
  return node;
}

export function describeSchema(
  document: OpenApiDocument,
  raw: unknown,
  name: string | null = null,
  required = false,
  trail: readonly string[] = [],
  depth = 0,
): SchemaNode {
  const normalized = normalizeSchema(document, raw, trail);
  if (normalized.recursive) {
    return trailNode(name, required, normalized.refName, 'recursive');
  }
  const { schema } = normalized;
  const properties = objectOf(schema.properties);
  const propertyNames = Object.keys(properties);
  const requiredNames = new Set(stringsOf(schema.required));
  const atLimit = depth >= MAX_SCHEMA_DEPTH;
  const type = primaryType(schema);
  const itemsRaw = schema.items;
  const items =
    !atLimit && isObject(itemsRaw)
      ? describeSchema(document, itemsRaw, null, false, normalized.trail, depth + 1)
      : null;
  const alternatives = atLimit
    ? []
    : normalized.alternatives.map((option) =>
        describeSchema(document, option, null, false, normalized.trail, depth + 1),
      );
  const additionalRaw = schema.additionalProperties;
  const additional =
    !atLimit && isObject(additionalRaw)
      ? describeSchema(document, additionalRaw, null, false, normalized.trail, depth + 1)
      : null;
  const hasProperties = propertyNames.length > 0;
  const expandable =
    !atLimit &&
    (hasProperties ||
      additional !== null ||
      alternatives.length > 0 ||
      (items !== null && (items.expandable || items.recursive || items.truncated)));
  const wouldExpand =
    hasProperties ||
    isObject(additionalRaw) ||
    normalized.alternatives.length > 0 ||
    (isObject(itemsRaw) && type === 'array');
  const node: SchemaNode = {
    name,
    required,
    refName: normalized.refName,
    type,
    format: stringOf(schema.format),
    nullable: normalized.nullable,
    description: stringOf(schema.description),
    enumValues: enumOf(schema),
    defaultValue: 'default' in schema ? formatValue(schema.default) : null,
    example: exampleOf(schema),
    constraints: constraintsOf(schema),
    readOnly: schema.readOnly === true,
    writeOnly: schema.writeOnly === true,
    deprecated: schema.deprecated === true,
    items,
    alternatives,
    additional,
    hasProperties: hasProperties && !atLimit,
    expandable,
    recursive: false,
    truncated: atLimit && wouldExpand,
    properties: atLimit
      ? emptyMemo()
      : memoize(() =>
          propertyNames.map((key) =>
            describeSchema(document, properties[key], key, requiredNames.has(key), normalized.trail, depth + 1),
          ),
        ),
  };
  return node;
}
