import { openApiFixture } from './fixtures/openapi';

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function schemaOf(name: string): UnknownRecord {
  const components = isRecord(openApiFixture) ? openApiFixture.components : undefined;
  const schemas = isRecord(components) ? components.schemas : undefined;
  const schema = isRecord(schemas) ? schemas[name] : undefined;
  if (!isRecord(schema)) {
    throw new Error(`The OpenAPI fixture has no schema ${name}.`);
  }
  return schema;
}

function collectProperties(schema: UnknownRecord): string[] {
  const own = isRecord(schema.properties) ? Object.keys(schema.properties) : [];
  const parts = Array.isArray(schema.allOf) ? schema.allOf : [];
  return [...own, ...parts.filter(isRecord).flatMap(collectProperties)];
}

function collectRequired(schema: UnknownRecord): string[] {
  const own = Array.isArray(schema.required) ? schema.required.filter((item) => typeof item === 'string') : [];
  const parts = Array.isArray(schema.allOf) ? schema.allOf : [];
  return [...own, ...parts.filter(isRecord).flatMap(collectRequired)];
}

export function schemaProperties(name: string): string[] {
  return collectProperties(schemaOf(name));
}

export function schemaRequired(name: string): string[] {
  return collectRequired(schemaOf(name));
}

export function unknownFieldsOf(name: string, body: UnknownRecord): string[] {
  const known = schemaProperties(name);
  return Object.keys(body).filter((key) => !known.includes(key));
}

export function missingFieldsOf(name: string, body: UnknownRecord): string[] {
  return schemaRequired(name).filter((key) => !(key in body));
}
