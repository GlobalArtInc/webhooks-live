import type {
  Capture,
  CaptureBodyOptions,
  CaptureListQuery,
  CapturePage,
  Endpoint,
  EndpointInput,
  EndpointListQuery,
  EndpointPage,
  FieldError,
  ForwardJob,
  ForwardRequest,
  ForwardRule,
  ForwardRuleInput,
  Instance,
  Member,
  MemberInput,
  MemberList,
} from './types';

export const API_PREFIX = '/api/v1';

type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;
type UnknownRecord = Record<string, unknown>;

export interface RateLimitInfo {
  limit: number | null;
  remaining: number | null;
}

export type RateLimitObserver = (info: RateLimitInfo) => void;

interface ApiErrorInit {
  status: number;
  code: string | null;
  message: string;
  details: FieldError[];
  retryAfter: number | null;
}

export class ApiError extends Error {
  readonly status: number;
  readonly code: string | null;
  readonly details: FieldError[];
  readonly retryAfter: number | null;

  constructor(init: ApiErrorInit) {
    super(init.message);
    this.name = 'ApiError';
    this.status = init.status;
    this.code = init.code;
    this.details = init.details;
    this.retryAfter = init.retryAfter;
  }
}

interface RequestOptions {
  method: HttpMethod;
  query?: QueryParams;
  body?: unknown;
}

interface ParsedError {
  code: string | null;
  message: string | null;
  fields: FieldError[];
}

let rateLimitObserver: RateLimitObserver | null = null;

export function setRateLimitObserver(observer: RateLimitObserver | null): void {
  rateLimitObserver = observer;
}

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function parseNonNegativeInteger(value: string | null): number | null {
  return value !== null && /^\d+$/.test(value.trim()) ? Number(value) : null;
}

function observeRateLimit(headers: Headers): void {
  const info: RateLimitInfo = {
    limit: parseNonNegativeInteger(headers.get('RateLimit-Limit')),
    remaining: parseNonNegativeInteger(headers.get('RateLimit-Remaining')),
  };
  if (rateLimitObserver !== null && (info.limit !== null || info.remaining !== null)) {
    rateLimitObserver(info);
  }
}

function readFieldErrors(value: unknown): FieldError[] {
  const items: unknown[] = Array.isArray(value) ? value : [];
  const fields: FieldError[] = [];
  for (const item of items) {
    if (isRecord(item) && typeof item.field === 'string' && typeof item.message === 'string') {
      const field: FieldError = { field: item.field, message: item.message };
      if (typeof item.code === 'string') {
        field.code = item.code;
      }
      fields.push(field);
    }
  }
  return fields;
}

function parseErrorBody(text: string): ParsedError | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return null;
  }
  const error = isRecord(parsed) ? parsed.error : undefined;
  if (!isRecord(error)) {
    return null;
  }
  return {
    code: typeof error.code === 'string' ? error.code : null,
    message: typeof error.message === 'string' ? error.message : null,
    fields: readFieldErrors(error.fields),
  };
}

async function readApiError(response: Response): Promise<ApiError> {
  const text = await response.text().catch(() => '');
  const parsed = parseErrorBody(text);
  return new ApiError({
    status: response.status,
    code: parsed?.code ?? null,
    message: parsed?.message ?? `Request failed with status ${response.status}`,
    details: parsed?.fields ?? [],
    retryAfter: parseNonNegativeInteger(response.headers.get('Retry-After')),
  });
}

function buildUrl(path: string, query: QueryParams | undefined): URL {
  const url = new URL(`${API_PREFIX}${path}`, window.location.origin);
  if (query !== undefined) {
    for (const [name, value] of Object.entries(query)) {
      if (value !== undefined && value !== null && value !== '') {
        url.searchParams.set(name, String(value));
      }
    }
  }
  return url;
}

async function send(path: string, options: RequestOptions): Promise<Response> {
  const headers = new Headers();
  let body: string | undefined;
  if (options.body !== undefined) {
    headers.set('Content-Type', 'application/json');
    body = JSON.stringify(options.body);
  }
  const response = await fetch(buildUrl(path, options.query), {
    method: options.method,
    headers,
    body,
    credentials: 'same-origin',
  });
  observeRateLimit(response.headers);
  if (!response.ok) {
    throw await readApiError(response);
  }
  return response;
}

export async function requestJson<T>(path: string, options: RequestOptions): Promise<T> {
  const response = await send(path, options);
  const payload: unknown = await response.json();
  return payload as T;
}

export async function requestEmpty(path: string, options: RequestOptions): Promise<void> {
  await send(path, options);
}

async function requestBlob(path: string, options: RequestOptions): Promise<Blob> {
  const response = await send(path, options);
  return response.blob();
}

function endpointPath(endpointId: string): string {
  return `/endpoints/${encodeURIComponent(endpointId)}`;
}

function capturePath(endpointId: string, captureId: string): string {
  return `${endpointPath(endpointId)}/captures/${encodeURIComponent(captureId)}`;
}

export function getInstance(): Promise<Instance> {
  return requestJson<Instance>('/instance', { method: 'GET' });
}

export function createEndpoint(input: EndpointInput = {}): Promise<Endpoint> {
  return requestJson<Endpoint>('/endpoints', { method: 'POST', body: input });
}

export function listEndpoints(query: EndpointListQuery = {}): Promise<EndpointPage> {
  return requestJson<EndpointPage>('/endpoints', { method: 'GET', query: { ...query } });
}

export function getEndpoint(endpointId: string): Promise<Endpoint> {
  return requestJson<Endpoint>(endpointPath(endpointId), { method: 'GET' });
}

export function patchEndpoint(endpointId: string, input: EndpointInput): Promise<Endpoint> {
  return requestJson<Endpoint>(endpointPath(endpointId), { method: 'PATCH', body: input });
}

export function deleteEndpoint(endpointId: string): Promise<void> {
  return requestEmpty(endpointPath(endpointId), { method: 'DELETE' });
}

export function listMembers(endpointId: string): Promise<MemberList> {
  return requestJson<MemberList>(`${endpointPath(endpointId)}/members`, { method: 'GET' });
}

export function putMember(endpointId: string, input: MemberInput): Promise<Member> {
  return requestJson<Member>(`${endpointPath(endpointId)}/members`, { method: 'PUT', body: input });
}

export function removeMember(endpointId: string, userId: string): Promise<void> {
  return requestEmpty(`${endpointPath(endpointId)}/members/${encodeURIComponent(userId)}`, { method: 'DELETE' });
}

export function listCaptures(endpointId: string, query: CaptureListQuery = {}): Promise<CapturePage> {
  return requestJson<CapturePage>(`${endpointPath(endpointId)}/captures`, { method: 'GET', query: { ...query } });
}

export function getLatestCapture(endpointId: string): Promise<Capture> {
  return requestJson<Capture>(`${endpointPath(endpointId)}/captures/latest`, { method: 'GET' });
}

export function getCapture(endpointId: string, captureId: string): Promise<Capture> {
  return requestJson<Capture>(capturePath(endpointId, captureId), { method: 'GET' });
}

export function fetchCaptureBody(
  endpointId: string,
  captureId: string,
  options: CaptureBodyOptions = {},
): Promise<Blob> {
  return requestBlob(`${capturePath(endpointId, captureId)}/body`, {
    method: 'GET',
    query: { view: options.view, download: options.download === true ? 1 : undefined },
  });
}

export function deleteCapture(endpointId: string, captureId: string): Promise<void> {
  return requestEmpty(capturePath(endpointId, captureId), { method: 'DELETE' });
}

export function clearCaptures(endpointId: string): Promise<void> {
  return requestEmpty(`${endpointPath(endpointId)}/captures`, { method: 'DELETE' });
}

export function resendCapture(endpointId: string, captureId: string, input: ForwardRequest): Promise<ForwardJob> {
  return requestJson<ForwardJob>(`${capturePath(endpointId, captureId)}/forward`, { method: 'POST', body: input });
}

export function createForwardRule(endpointId: string, input: ForwardRuleInput): Promise<ForwardRule> {
  return requestJson<ForwardRule>(`${endpointPath(endpointId)}/forward-rules`, { method: 'POST', body: input });
}

export function patchForwardRule(
  endpointId: string,
  ruleId: string,
  input: Partial<ForwardRuleInput>,
): Promise<ForwardRule> {
  return requestJson<ForwardRule>(`${endpointPath(endpointId)}/forward-rules/${encodeURIComponent(ruleId)}`, {
    method: 'PATCH',
    body: input,
  });
}

export function deleteForwardRule(endpointId: string, ruleId: string): Promise<void> {
  return requestEmpty(`${endpointPath(endpointId)}/forward-rules/${encodeURIComponent(ruleId)}`, { method: 'DELETE' });
}
