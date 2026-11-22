import { buildBodySkeleton } from './bodySkeleton';
import type { OpenApiDocument } from './document';
import { arrayOf, formatValue, isObject, objectOf, stringOf, type JsonObject } from './json';
import { resolveObjectRef } from './schemaResolve';
import { describeSchema, type SchemaNode } from './schemaTree';

export type ParameterLocation = 'path' | 'query' | 'header' | 'cookie';
export type SecurityKind = 'session' | 'token' | 'public' | 'other';

export interface ReferenceParameter {
  name: string;
  location: ParameterLocation;
  required: boolean;
  deprecated: boolean;
  description: string | null;
  schema: SchemaNode | null;
  example: string | null;
}

export interface ReferenceMedia {
  mediaType: string;
  schema: SchemaNode | null;
  skeleton: string | null;
}

export interface ReferenceBody {
  required: boolean;
  description: string | null;
  content: ReferenceMedia[];
}

export interface ReferenceHeader {
  name: string;
  description: string | null;
  schema: SchemaNode | null;
}

export interface ReferenceResponse {
  status: string;
  description: string | null;
  content: ReferenceMedia[];
  headers: ReferenceHeader[];
}

export interface SecurityChip {
  kind: SecurityKind;
  name: string;
}

export interface ReferenceSecurityScheme extends SecurityChip {
  description: string | null;
}

export interface ReferenceOperation {
  id: string;
  method: string;
  path: string;
  summary: string | null;
  description: string | null;
  tag: string | null;
  deprecated: boolean;
  security: SecurityChip[];
  acceptsToken: boolean;
  parameters: ReferenceParameter[];
  requestBody: ReferenceBody | null;
  responses: ReferenceResponse[];
  searchText: string;
}

export interface ReferenceTag {
  name: string | null;
  description: string | null;
  operations: ReferenceOperation[];
}

export interface Reference {
  title: string | null;
  version: string | null;
  description: string | null;
  serverUrl: string;
  securitySchemes: ReferenceSecurityScheme[];
  tags: ReferenceTag[];
  operations: ReferenceOperation[];
}

const METHODS: readonly string[] = ['get', 'put', 'post', 'delete', 'options', 'head', 'patch', 'trace'];
const LOCATIONS: readonly string[] = ['path', 'query', 'header', 'cookie'];
const DEFAULT_SERVER_URL = '/api/v1';
const DEFAULT_STATUS_RANK = 1000;
const UNKNOWN_STATUS_RANK = 999;

function jsonLike(mediaType: string): boolean {
  const base = mediaType.split(';')[0]?.trim().toLowerCase() ?? '';
  return base === 'application/json' || base.endsWith('+json') || base === 'application/x-ndjson';
}

function exampleText(parameter: JsonObject, schema: JsonObject): string | null {
  const candidates: unknown[] = [parameter.example, schema.example, arrayOf(schema.enum)[0]];
  const found = candidates.find((candidate) => typeof candidate === 'string' || typeof candidate === 'number');
  return found === undefined ? null : formatValue(found).replace(/^"|"$/g, '');
}

function mediaOf(document: OpenApiDocument, content: unknown, withSkeleton: boolean): ReferenceMedia[] {
  return Object.entries(objectOf(content)).map(([mediaType, entry]) => {
    const media = objectOf(entry);
    const hasSchema = media.schema !== undefined;
    const skeleton =
      withSkeleton && hasSchema && jsonLike(mediaType)
        ? JSON.stringify(buildBodySkeleton(document, media.schema), null, 2)
        : null;
    return {
      mediaType,
      schema: hasSchema ? describeSchema(document, media.schema) : null,
      skeleton,
    };
  });
}

function parameterOf(document: OpenApiDocument, raw: unknown): ReferenceParameter | null {
  const parameter = resolveObjectRef(document, raw);
  const name = stringOf(parameter.name);
  const location = stringOf(parameter.in);
  if (name === null || location === null || !LOCATIONS.includes(location)) {
    return null;
  }
  const contentSchema = Object.values(objectOf(parameter.content)).map((entry) => objectOf(entry).schema)[0];
  const rawSchema = parameter.schema ?? contentSchema;
  const schema = rawSchema === undefined ? null : describeSchema(document, rawSchema);
  return {
    name,
    location: location as ParameterLocation,
    required: parameter.required === true || location === 'path',
    deprecated: parameter.deprecated === true,
    description: stringOf(parameter.description),
    schema,
    example: exampleText(parameter, objectOf(rawSchema)),
  };
}

function parametersOf(document: OpenApiDocument, pathLevel: unknown, operationLevel: unknown): ReferenceParameter[] {
  const merged = new Map<string, ReferenceParameter>();
  for (const raw of [...arrayOf(pathLevel), ...arrayOf(operationLevel)]) {
    const parameter = parameterOf(document, raw);
    if (parameter !== null) {
      merged.set(`${parameter.location}:${parameter.name}`, parameter);
    }
  }
  return [...merged.values()];
}

function bodyOf(document: OpenApiDocument, raw: unknown): ReferenceBody | null {
  if (raw === undefined) {
    return null;
  }
  const body = resolveObjectRef(document, raw);
  return {
    required: body.required === true,
    description: stringOf(body.description),
    content: mediaOf(document, body.content, true),
  };
}

function headersOf(document: OpenApiDocument, raw: unknown): ReferenceHeader[] {
  return Object.entries(objectOf(raw)).map(([name, entry]) => {
    const header = resolveObjectRef(document, entry);
    return {
      name,
      description: stringOf(header.description),
      schema: header.schema === undefined ? null : describeSchema(document, header.schema),
    };
  });
}

function statusRank(status: string): number {
  if (status === 'default') {
    return DEFAULT_STATUS_RANK;
  }
  const value = Number.parseInt(status, 10);
  return Number.isNaN(value) ? UNKNOWN_STATUS_RANK : value;
}

function responsesOf(document: OpenApiDocument, raw: unknown): ReferenceResponse[] {
  return Object.entries(objectOf(raw))
    .map(([status, entry]) => {
      const response = resolveObjectRef(document, entry);
      return {
        status,
        description: stringOf(response.description),
        content: mediaOf(document, response.content, false),
        headers: headersOf(document, response.headers),
      };
    })
    .sort((left, right) => statusRank(left.status) - statusRank(right.status));
}

function schemeKind(name: string, scheme: JsonObject): SecurityKind {
  const type = stringOf(scheme.type);
  if (type === 'http' && stringOf(scheme.scheme)?.toLowerCase() === 'bearer') {
    return 'token';
  }
  if (type === 'apiKey' && stringOf(scheme.in) === 'cookie') {
    return 'session';
  }
  if (type === null) {
    return name === 'api_token' ? 'token' : name === 'session_cookie' ? 'session' : 'other';
  }
  return 'other';
}

function securitySchemesOf(document: OpenApiDocument): Map<string, ReferenceSecurityScheme> {
  const schemes = objectOf(objectOf(document.components).securitySchemes);
  const result = new Map<string, ReferenceSecurityScheme>();
  for (const [name, entry] of Object.entries(schemes)) {
    const scheme = resolveObjectRef(document, entry);
    result.set(name, { name, kind: schemeKind(name, scheme), description: stringOf(scheme.description) });
  }
  return result;
}

function securityChipsOf(requirements: unknown, schemes: Map<string, ReferenceSecurityScheme>): SecurityChip[] {
  const list = Array.isArray(requirements) ? requirements : [];
  const chips = new Map<string, SecurityChip>();
  function add(chip: SecurityChip): void {
    chips.set(`${chip.kind}:${chip.name}`, chip);
  }
  if (list.length === 0) {
    add({ kind: 'public', name: '' });
  }
  for (const requirement of list) {
    const names = Object.keys(objectOf(requirement));
    if (names.length === 0) {
      add({ kind: 'public', name: '' });
    }
    for (const name of names) {
      const known = schemes.get(name);
      add({ kind: known?.kind ?? schemeKind(name, {}), name });
    }
  }
  return [...chips.values()];
}

function slug(method: string, path: string): string {
  const text = `${method}_${path}`.toLowerCase().replace(/[^a-z0-9]+/g, '_');
  return text.replace(/^_+|_+$/g, '');
}

function serverUrlOf(document: OpenApiDocument): string {
  const [first] = arrayOf(document.servers);
  const server = objectOf(first);
  const url = stringOf(server.url);
  if (url === null) {
    return DEFAULT_SERVER_URL;
  }
  const variables = objectOf(server.variables);
  return url.replace(/\{([^}]+)\}/g, (match, name: string) => stringOf(objectOf(variables[name]).default) ?? match);
}

function operationsOf(document: OpenApiDocument, schemes: Map<string, ReferenceSecurityScheme>): ReferenceOperation[] {
  const used = new Set<string>();
  const operations: ReferenceOperation[] = [];
  const globalSecurity = document.security;
  for (const [path, entry] of Object.entries(document.paths)) {
    if (!isObject(entry)) {
      continue;
    }
    for (const method of METHODS) {
      const raw = entry[method];
      if (!isObject(raw)) {
        continue;
      }
      const declared = stringOf(raw.operationId);
      let id = declared !== null && declared !== '' ? declared : slug(method, path);
      for (let suffix = 2; used.has(id); suffix += 1) {
        id = `${declared ?? slug(method, path)}_${suffix}`;
      }
      used.add(id);
      const security = securityChipsOf(raw.security ?? globalSecurity, schemes);
      const summary = stringOf(raw.summary);
      const description = stringOf(raw.description);
      const [tag] = arrayOf(raw.tags).filter((item): item is string => typeof item === 'string');
      operations.push({
        id,
        method: method.toUpperCase(),
        path,
        summary,
        description,
        tag: tag ?? null,
        deprecated: raw.deprecated === true,
        security,
        acceptsToken: security.some((chip) => chip.kind === 'token'),
        parameters: parametersOf(document, entry.parameters, raw.parameters),
        requestBody: bodyOf(document, raw.requestBody),
        responses: responsesOf(document, raw.responses),
        searchText: [method, path, id, summary ?? ''].join(' ').toLowerCase(),
      });
    }
  }
  return operations;
}

function tagsOf(document: OpenApiDocument, operations: ReferenceOperation[]): ReferenceTag[] {
  const described = new Map<string, string | null>();
  for (const entry of arrayOf(document.tags)) {
    const name = stringOf(objectOf(entry).name);
    if (name !== null) {
      described.set(name, stringOf(objectOf(entry).description));
    }
  }
  const order: (string | null)[] = [...described.keys()];
  for (const operation of operations) {
    if (operation.tag !== null && !order.includes(operation.tag)) {
      order.push(operation.tag);
    }
  }
  if (operations.some((operation) => operation.tag === null)) {
    order.push(null);
  }
  return order
    .map((name) => ({
      name,
      description: name === null ? null : (described.get(name) ?? null),
      operations: operations.filter((operation) => operation.tag === name),
    }))
    .filter((tag) => tag.operations.length > 0);
}

export function buildReference(document: OpenApiDocument): Reference {
  const schemes = securitySchemesOf(document);
  const operations = operationsOf(document, schemes);
  const info = objectOf(document.info);
  return {
    title: stringOf(info.title),
    version: stringOf(info.version),
    description: stringOf(info.description),
    serverUrl: serverUrlOf(document),
    securitySchemes: [...schemes.values()],
    tags: tagsOf(document, operations),
    operations,
  };
}

export function operationLabel(operation: ReferenceOperation): string {
  return `${operation.method} ${operation.path}`;
}
