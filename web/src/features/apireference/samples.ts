import type { ReferenceMedia, ReferenceOperation, ReferenceParameter } from './reference';

export interface SampleContext {
  origin: string;
  serverUrl: string;
}

export interface SampleHeader {
  name: string;
  value: string;
}

export interface OperationSample {
  method: string;
  url: string;
  host: string;
  target: string;
  headers: SampleHeader[];
  body: string | null;
}

export const TOKEN_PLACEHOLDER = '<token>';
export const TOKEN_VARIABLE = '$WEBHOOKS_LIVE_TOKEN';

const PATH_PARAMETER = /\{([^}]+)\}/g;
const JSON_MEDIA = 'application/json';
const NDJSON_MEDIA = 'application/x-ndjson';
const BODY_PLACEHOLDER = '<body>';

interface ServerBase {
  origin: string;
  path: string;
}

function serverBase(context: SampleContext): ServerBase {
  try {
    const resolved = new URL(context.serverUrl, context.origin);
    return { origin: resolved.origin, path: resolved.pathname.replace(/\/+$/, '') };
  } catch {
    return { origin: context.origin.replace(/\/+$/, ''), path: context.serverUrl.replace(/\/+$/, '') };
  }
}

function placeholder(name: string): string {
  return `<${name}>`;
}

function valueOf(parameter: ReferenceParameter | undefined, name: string): string {
  const example = parameter?.example ?? null;
  return example === null ? placeholder(name) : encodeURIComponent(example);
}

function pathOf(operation: ReferenceOperation): string {
  return operation.path.replace(PATH_PARAMETER, (_match, name: string) =>
    valueOf(
      operation.parameters.find((parameter) => parameter.location === 'path' && parameter.name === name),
      name,
    ),
  );
}

function queryOf(operation: ReferenceOperation): string {
  return operation.parameters
    .filter((parameter) => parameter.location === 'query' && parameter.required)
    .map((parameter) => `${encodeURIComponent(parameter.name)}=${valueOf(parameter, parameter.name)}`)
    .join('&');
}

function chooseMedia(content: readonly ReferenceMedia[]): ReferenceMedia | null {
  const byType = (type: string) => content.find((media) => media.mediaType.split(';')[0]?.trim() === type);
  return byType(JSON_MEDIA) ?? content.find((media) => media.skeleton !== null) ?? content[0] ?? null;
}

function compactJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text));
  } catch {
    return text;
  }
}

function bodyHeaderAndText(operation: ReferenceOperation): { header: SampleHeader | null; body: string | null } {
  const media = operation.requestBody === null ? null : chooseMedia(operation.requestBody.content);
  if (media === null) {
    return { header: null, body: null };
  }
  const mediaType = media.mediaType.split(';')[0]?.trim() ?? media.mediaType;
  const header = { name: 'Content-Type', value: mediaType };
  if (media.skeleton === null) {
    return { header, body: BODY_PLACEHOLDER };
  }
  return { header, body: mediaType === NDJSON_MEDIA ? compactJson(media.skeleton) : media.skeleton };
}

export function buildSample(operation: ReferenceOperation, context: SampleContext): OperationSample {
  const base = serverBase(context);
  const query = queryOf(operation);
  const target = `${base.path}${pathOf(operation)}${query === '' ? '' : `?${query}`}`;
  const headers: SampleHeader[] = [];
  if (operation.acceptsToken) {
    headers.push({ name: 'Authorization', value: `Bearer ${TOKEN_PLACEHOLDER}` });
  }
  for (const parameter of operation.parameters) {
    if (parameter.location === 'header' && parameter.required) {
      headers.push({ name: parameter.name, value: parameter.example ?? placeholder(parameter.name) });
    }
  }
  const { header, body } = bodyHeaderAndText(operation);
  if (header !== null) {
    headers.push(header);
  }
  return {
    method: operation.method,
    url: `${base.origin}${target}`,
    host: new URL(base.origin, context.origin).host,
    target,
    headers,
    body,
  };
}

function shellQuote(text: string): string {
  return `'${text.replaceAll("'", "'\\''")}'`;
}

function curlHeader(header: SampleHeader): string {
  const value = header.value.replace(TOKEN_PLACEHOLDER, TOKEN_VARIABLE);
  return value.includes('$')
    ? `--header "${header.name}: ${value.replaceAll('"', '\\"')}"`
    : `--header ${shellQuote(`${header.name}: ${value}`)}`;
}

export function formatCurl(sample: OperationSample): string {
  const parts = [sample.method === 'HEAD' ? 'curl --head' : `curl --request ${sample.method}`];
  parts.push(`--url ${shellQuote(sample.url)}`);
  for (const header of sample.headers) {
    parts.push(curlHeader(header));
  }
  if (sample.body !== null) {
    parts.push(`--data ${shellQuote(sample.body)}`);
  }
  return parts.join(' \\\n  ');
}

export function formatHttp(sample: OperationSample): string {
  const lines = [`${sample.method} ${sample.target} HTTP/1.1`, `Host: ${sample.host}`];
  for (const header of sample.headers) {
    lines.push(`${header.name}: ${header.value}`);
  }
  const head = lines.join('\n');
  return sample.body === null ? head : `${head}\n\n${sample.body}`;
}
