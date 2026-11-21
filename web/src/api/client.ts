import type {
  ApiLimit,
  Capture,
  CaptureBodyOptions,
  CaptureListQuery,
  CapturePage,
  CaptureSummary,
  Endpoint,
  EndpointInput,
  EndpointListQuery,
  EndpointPage,
  CaptureExportFile,
  CaptureExportInput,
  CaptureExportQuery,
  FieldError,
  ForwardJob,
  ForwardRequest,
  ForwardRule,
  ForwardRuleInput,
  Folder,
  FolderList,
  FolderPatch,
  InboxCaptureQuery,
  Instance,
  Member,
  MemberInput,
  MemberList,
  SendRequest,
  SendResult,
  SentRequestPage,
  SentRequestQuery,
  Page,
} from './types';

export const API_PREFIX = '/api/v1';

export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
type QueryValue = string | number | boolean | null | undefined;
type QueryParams = Record<string, QueryValue>;
type UnknownRecord = Record<string, unknown>;

export interface RateLimitInfo {
  limit: number | null;
  remaining: number | null;
}

export type RateLimitObserver = (info: RateLimitInfo) => void;

export type GrantResolver = (endpointId: string) => string | null;
export type GrantRejectedHandler = (endpointId: string) => void;

interface ApiErrorInit {
  status: number;
  code: string | null;
  message: string;
  details: FieldError[];
  retryAfter: number | null;
  limit?: ApiLimit | null;
}

export class ApiError extends Error {
  readonly status: number;
  readonly code: string | null;
  readonly details: FieldError[];
  readonly retryAfter: number | null;
  readonly limit: ApiLimit | null;

  constructor(init: ApiErrorInit) {
    super(init.message);
    this.name = 'ApiError';
    this.status = init.status;
    this.code = init.code;
    this.details = init.details;
    this.retryAfter = init.retryAfter;
    this.limit = init.limit ?? null;
  }
}

export interface RequestOptions {
  method: HttpMethod;
  query?: QueryParams;
  body?: unknown;
  signal?: AbortSignal;
}

interface ParsedError {
  code: string | null;
  message: string | null;
  fields: FieldError[];
  limit: ApiLimit | null;
}

export const GRANT_HEADER = 'X-Webhooks-Live-Grant';
const ENDPOINT_PATH = /^\/endpoints\/([^/]+)/;

let rateLimitObserver: RateLimitObserver | null = null;
let grantResolver: GrantResolver | null = null;
let grantRejectedHandler: GrantRejectedHandler | null = null;

export function setRateLimitObserver(observer: RateLimitObserver | null): void {
  rateLimitObserver = observer;
}

export function setGrantResolver(resolver: GrantResolver | null): void {
  grantResolver = resolver;
}

export function setGrantRejectedHandler(handler: GrantRejectedHandler | null): void {
  grantRejectedHandler = handler;
}

export function reportGrantRejected(endpointId: string): void {
  grantRejectedHandler?.(endpointId);
}

function reportRejectedGrant(path: string, error: ApiError): void {
  const endpointId = ENDPOINT_PATH.exec(path)?.[1];
  if (endpointId !== undefined && error.status === 401 && error.code === 'grant_required') {
    reportGrantRejected(endpointId);
  }
}

function grantFor(path: string): string | null {
  const endpointId = ENDPOINT_PATH.exec(path)?.[1];
  return grantResolver === null || endpointId === undefined ? null : grantResolver(endpointId);
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

function readLimit(value: unknown): ApiLimit | null {
  if (!isRecord(value) || typeof value.name !== 'string' || typeof value.max !== 'number') {
    return null;
  }
  return {
    name: value.name,
    max: value.max,
    window: typeof value.window === 'string' ? value.window : null,
    resets_at: typeof value.resets_at === 'string' ? value.resets_at : null,
  };
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
    limit: readLimit(error.limit),
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
    limit: parsed?.limit ?? null,
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
  const grant = grantFor(path);
  if (grant !== null) {
    headers.set(GRANT_HEADER, grant);
  }
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
    signal: options.signal,
  });
  observeRateLimit(response.headers);
  if (!response.ok) {
    const error = await readApiError(response);
    reportRejectedGrant(path, error);
    throw error;
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

export function endpointPath(endpointId: string): string {
  return `/endpoints/${encodeURIComponent(endpointId)}`;
}

function folderPath(folderId: string): string {
  return `/folders/${encodeURIComponent(folderId)}`;
}

function folderEndpointPath(folderId: string, endpointId: string): string {
  return `${folderPath(folderId)}/endpoints/${encodeURIComponent(endpointId)}`;
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

export function claimEndpoint(endpointId: string): Promise<Endpoint> {
  return requestJson<Endpoint>(`${endpointPath(endpointId)}/claim`, { method: 'POST' });
}

export function listFolders(): Promise<FolderList> {
  return requestJson<FolderList>('/folders', { method: 'GET' });
}

export function createFolder(name: string): Promise<Folder> {
  return requestJson<Folder>('/folders', { method: 'POST', body: { name } });
}

export function patchFolder(folderId: string, input: FolderPatch): Promise<Folder> {
  return requestJson<Folder>(folderPath(folderId), { method: 'PATCH', body: input });
}

export function deleteFolder(folderId: string): Promise<void> {
  return requestEmpty(folderPath(folderId), { method: 'DELETE' });
}

export function putFolderEndpoint(folderId: string, endpointId: string): Promise<void> {
  return requestEmpty(folderEndpointPath(folderId, endpointId), { method: 'PUT' });
}

export function deleteFolderEndpoint(folderId: string, endpointId: string): Promise<void> {
  return requestEmpty(folderEndpointPath(folderId, endpointId), { method: 'DELETE' });
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

export function captureExportPath(endpointId: string, query: CaptureExportQuery): string {
  const url = buildUrl(`${endpointPath(endpointId)}/export`, { ...query });
  return `${url.pathname}${url.search}`;
}

export async function createCaptureExport(
  endpointId: string,
  input: CaptureExportInput,
  signal?: AbortSignal,
): Promise<CaptureExportFile> {
  const response = await send(`${endpointPath(endpointId)}/export`, { method: 'POST', body: input, signal });
  return { blob: await response.blob(), contentDisposition: response.headers.get('Content-Disposition') };
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

export function sendRequest(input: SendRequest, signal?: AbortSignal): Promise<SendResult> {
  return requestJson<SendResult>('/send', { method: 'POST', body: input, signal });
}

export function listSentRequests(query: SentRequestQuery = {}): Promise<SentRequestPage> {
  return requestJson<SentRequestPage>('/send/history', { method: 'GET', query: { ...query } });
}

export function deleteSentRequest(sentId: string): Promise<void> {
  return requestEmpty(`/send/history/${encodeURIComponent(sentId)}`, { method: 'DELETE' });
}

export function listInboxCaptures(query: InboxCaptureQuery = {}): Promise<Page<CaptureSummary>> {
  return requestJson<Page<CaptureSummary>>('/inbox/captures', { method: 'GET', query: { ...query } });
}

