import { API_PREFIX, ApiError } from '../../api/client';
import type { Body, NameValue } from '../../api/types';
import { DEFAULT_DELIMITERS } from '../editor/constants';
import { SAMPLE_BODY } from '../endpoint/sampleCurl';
import type {
  CatalogueVariable,
  PreviewDiagnostic,
  PreviewRequest,
  PreviewResult,
  PreviewSeverity,
  RenderedResponse,
  VariableCatalogue,
} from './types';

type UnknownRecord = Record<string, unknown>;

const SAMPLE_CONTENT_TYPE = 'application/json';
const REFUSING_STATUSES: readonly number[] = [410, 413, 429];

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function textOf(value: unknown, fallback: string): string {
  return typeof value === 'string' ? value : fallback;
}

function nullableText(value: unknown): string | null {
  return typeof value === 'string' ? value : null;
}

function positiveInteger(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 1 ? Math.floor(value) : fallback;
}

function nonNegativeInteger(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? Math.floor(value) : 0;
}

function endpointPath(endpointId: string): string {
  return `${API_PREFIX}/endpoints/${encodeURIComponent(endpointId)}`;
}

async function failureOf(response: Response): Promise<ApiError> {
  const text = await response.text().catch(() => '');
  let code: string | null = null;
  let message = `Request failed with status ${response.status}`;
  try {
    const parsed: unknown = JSON.parse(text);
    const error = isRecord(parsed) ? parsed.error : undefined;
    if (isRecord(error)) {
      code = typeof error.code === 'string' ? error.code : null;
      message = typeof error.message === 'string' ? error.message : message;
    }
  } catch (parseFailure: unknown) {
    if (!(parseFailure instanceof SyntaxError)) {
      throw parseFailure;
    }
  }
  return new ApiError({ status: response.status, code, message, details: [], retryAfter: null });
}

async function requestJson(url: URL, init: RequestInit): Promise<unknown> {
  const response = await fetch(url, { ...init, credentials: 'same-origin' });
  if (!response.ok) {
    throw await failureOf(response);
  }
  return response.json();
}

function readVariable(value: unknown): CatalogueVariable | null {
  if (!isRecord(value) || typeof value.path !== 'string' || typeof value.insert_text !== 'string') {
    return null;
  }
  return {
    path: value.path,
    insert_text: value.insert_text,
    group: textOf(value.group, 'request'),
    type: nullableText(value.type),
    sample: nullableText(value.sample),
    description: nullableText(value.description),
  };
}

function isVariable(value: CatalogueVariable | null): value is CatalogueVariable {
  return value !== null;
}

export function parseCatalogue(payload: unknown): VariableCatalogue {
  const record = isRecord(payload) ? payload : {};
  const delimiters = isRecord(record.delimiters) ? record.delimiters : {};
  const items: unknown[] = Array.isArray(record.items) ? record.items : [];
  return {
    delimiters: {
      open: textOf(delimiters.open, DEFAULT_DELIMITERS.open),
      close: textOf(delimiters.close, DEFAULT_DELIMITERS.close),
    },
    items: items.map(readVariable).filter(isVariable),
  };
}

function readSeverity(value: unknown): PreviewSeverity {
  return value === 'warning' ? 'warning' : 'error';
}

function readDiagnostic(value: unknown): PreviewDiagnostic | null {
  if (!isRecord(value) || typeof value.message !== 'string') {
    return null;
  }
  return {
    field: textOf(value.field, 'body'),
    code: textOf(value.code, 'template_invalid'),
    severity: readSeverity(value.severity),
    line: positiveInteger(value.line, 1),
    column: positiveInteger(value.column, 1),
    length: positiveInteger(value.length, 1),
    message: value.message,
  };
}

function isDiagnostic(value: PreviewDiagnostic | null): value is PreviewDiagnostic {
  return value !== null;
}

function readHeaders(value: unknown): NameValue[] {
  const items: unknown[] = Array.isArray(value) ? value : [];
  const headers: NameValue[] = [];
  for (const item of items) {
    if (isRecord(item) && typeof item.name === 'string' && typeof item.value === 'string') {
      headers.push({ name: item.name, value: item.value });
    }
  }
  return headers;
}

function readBody(value: unknown): Body {
  const record = isRecord(value) ? value : {};
  return {
    encoding: record.encoding === 'base64' ? 'base64' : 'utf8',
    data: textOf(record.data, ''),
    truncated: record.truncated === true,
  };
}

function readRendered(value: unknown): RenderedResponse | null {
  if (!isRecord(value) || typeof value.status !== 'number') {
    return null;
  }
  return {
    status: value.status,
    content_type: textOf(value.content_type, ''),
    headers: readHeaders(value.headers),
    body: readBody(value.body),
    body_size: nonNegativeInteger(value.body_size),
    duration_ms: nonNegativeInteger(value.duration_ms),
  };
}

export function parsePreview(payload: unknown): PreviewResult {
  const record = isRecord(payload) ? payload : {};
  const diagnostics: unknown[] = Array.isArray(record.diagnostics) ? record.diagnostics : [];
  return {
    capture_id: nullableText(record.capture_id),
    rendered: readRendered(record.rendered),
    diagnostics: diagnostics.map(readDiagnostic).filter(isDiagnostic),
  };
}

export async function fetchVariableCatalogue(
  endpointId: string,
  captureId: string | null,
  signal?: AbortSignal,
): Promise<VariableCatalogue> {
  const url = new URL(`${endpointPath(endpointId)}/template/variables`, window.location.origin);
  if (captureId !== null) {
    url.searchParams.set('capture_id', captureId);
  }
  return parseCatalogue(await requestJson(url, { method: 'GET', signal }));
}

export async function renderPreview(
  endpointId: string,
  request: PreviewRequest,
  signal: AbortSignal,
): Promise<PreviewResult> {
  const url = new URL(`${endpointPath(endpointId)}/template/preview`, window.location.origin);
  const payload = await requestJson(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
    signal,
  });
  return parsePreview(payload);
}

export async function sendSampleRequest(captureUrl: string): Promise<void> {
  const target = new URL(captureUrl, window.location.origin);
  const response = await fetch(`${target.pathname}${target.search}`, {
    method: 'POST',
    headers: { 'Content-Type': SAMPLE_CONTENT_TYPE },
    body: SAMPLE_BODY,
    credentials: 'omit',
  });
  if (REFUSING_STATUSES.includes(response.status)) {
    throw await failureOf(response);
  }
}
