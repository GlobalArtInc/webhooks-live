import type { Body, BodyEncoding, Capture, NameValue, SendRequest, SentRequest } from '../../api/types';
import { createKeyValueRow, type KeyValueRow } from '../../components/KeyValueEditor/rows';
import type { EditorLanguage } from '../editor/types';

export const METHODS: readonly string[] = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'];
export const CONTENT_TYPE_PRESETS: readonly string[] = [
  'application/json',
  'application/x-www-form-urlencoded',
  'text/plain',
  'application/xml',
  'text/html',
];
export const TIMEOUT_MAX_MS = 15000;
export const BODY_MAX_BYTES = 1572864;
export const REDACTED_VALUE = '[redacted]';

const CONTENT_TYPE_NAME = 'content-type';
const CONTENT_TYPE_LABEL = 'Content-Type';
const TIMEOUT_PATTERN = /^\d+$/;
const NOT_REPLAYED_HEADERS: ReadonlySet<string> = new Set([
  'connection',
  'keep-alive',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailer',
  'transfer-encoding',
  'upgrade',
  'host',
  'content-length',
]);

export interface DraftBody {
  encoding: BodyEncoding;
  data: string;
  truncated: boolean;
}

export interface ComposerDraft {
  method: string;
  url: string;
  headers: KeyValueRow[];
  body: DraftBody;
  timeout: string;
  followRedirects: boolean;
}

export interface ReplayBanner {
  method: string;
  target: string;
}

export interface ComposerSeed {
  draft: ComposerDraft;
  banner: ReplayBanner | null;
  sourceCaptureId: string | null;
}

export interface SendContext {
  endpointId: string | null;
  sourceCaptureId: string | null;
}

const EMPTY_BODY: DraftBody = { encoding: 'utf8', data: '', truncated: false };

export function emptyDraft(): ComposerDraft {
  return { method: 'GET', url: '', headers: [], body: EMPTY_BODY, timeout: '', followRedirects: false };
}

function pathWithQuery(path: string, query: string): string {
  return query === '' ? path : `${path}?${query}`;
}

function draftBodyOf(body: Body | null): DraftBody {
  return body === null ? EMPTY_BODY : { encoding: body.encoding, data: body.data, truncated: body.truncated };
}

function replayableHeaders(headers: readonly NameValue[]): KeyValueRow[] {
  return headers
    .filter((header) => !NOT_REPLAYED_HEADERS.has(header.name.toLowerCase()))
    .map((header) => createKeyValueRow(header.name, header.value));
}

export function seedFromCapture(capture: Capture): ComposerSeed {
  const target = pathWithQuery(capture.path, capture.query_string);
  return {
    draft: {
      method: capture.method,
      url: target,
      headers: replayableHeaders(capture.headers),
      body: draftBodyOf(capture.body),
      timeout: '',
      followRedirects: false,
    },
    banner: { method: capture.method, target },
    sourceCaptureId: capture.id,
  };
}

export function seedFromSent(entry: SentRequest): ComposerSeed {
  const { request } = entry;
  return {
    draft: {
      method: request.method,
      url: request.url,
      headers: request.headers.map((header) => createKeyValueRow(header.name, header.value)),
      body: draftBodyOf(request.body),
      timeout: request.timeout_ms === undefined ? '' : String(request.timeout_ms),
      followRedirects: request.follow_redirects ?? false,
    },
    banner: { method: request.method, target: request.url },
    sourceCaptureId: request.source_capture_id ?? null,
  };
}

export function parseTimeout(text: string): number | null {
  const value = text.trim();
  if (!TIMEOUT_PATTERN.test(value)) {
    return null;
  }
  const milliseconds = Number(value);
  return milliseconds < 1 ? null : Math.min(milliseconds, TIMEOUT_MAX_MS);
}

export function redactedRowIds(headers: readonly KeyValueRow[]): string[] {
  return headers.filter((row) => row.value.trim() === REDACTED_VALUE).map((row) => row.id);
}

export function buildSendRequest(draft: ComposerDraft, context: SendContext): SendRequest {
  const request: SendRequest = {
    method: draft.method,
    url: draft.url.trim(),
    headers: draft.headers
      .filter((row) => row.name.trim() !== '')
      .map((row) => ({ name: row.name.trim(), value: row.value })),
    body: draft.body.data === '' ? null : { encoding: draft.body.encoding, data: draft.body.data },
    follow_redirects: draft.followRedirects,
  };
  const timeout = parseTimeout(draft.timeout);
  if (timeout !== null) {
    request.timeout_ms = timeout;
  }
  if (context.endpointId !== null) {
    request.endpoint_id = context.endpointId;
  }
  if (context.sourceCaptureId !== null) {
    request.source_capture_id = context.sourceCaptureId;
  }
  return request;
}

function isContentTypeRow(row: KeyValueRow): boolean {
  return row.name.trim().toLowerCase() === CONTENT_TYPE_NAME;
}

export function contentTypeOf(headers: readonly KeyValueRow[]): string {
  const row = headers.find(isContentTypeRow);
  return row === undefined ? '' : row.value.trim();
}

export function contentTypeOptions(contentType: string): string[] {
  const custom = contentType !== '' && !CONTENT_TYPE_PRESETS.includes(contentType) ? [contentType] : [];
  return ['', ...CONTENT_TYPE_PRESETS, ...custom];
}

export function withContentType(headers: readonly KeyValueRow[], value: string): KeyValueRow[] {
  if (value === '') {
    return headers.filter((row) => !isContentTypeRow(row));
  }
  const index = headers.findIndex(isContentTypeRow);
  if (index === -1) {
    return [...headers, createKeyValueRow(CONTENT_TYPE_LABEL, value)];
  }
  return headers.map((row, position) => (position === index ? { ...row, value } : row));
}

export function languageOf(contentType: string): EditorLanguage {
  const type = contentType.toLowerCase();
  if (type.includes('json')) {
    return 'json';
  }
  if (type.includes('xml')) {
    return 'xml';
  }
  if (type.includes('html')) {
    return 'html';
  }
  return 'text';
}

export function methodOptions(method: string): string[] {
  return METHODS.includes(method) ? [...METHODS] : [...METHODS, method];
}
