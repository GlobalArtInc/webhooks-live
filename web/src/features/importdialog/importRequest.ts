import { ApiError, API_PREFIX } from '../../api/client';
import type { FieldError } from '../../api/types';
import { isRecord, parseRecord, type ImportFormat, type UnknownRecord } from './importFile';

export type ImportReason = 'capacity' | 'storage';

export interface ImportOutcome {
  imported: number | null;
  skipped: number | null;
  reason: ImportReason | null;
  endpointId: string | null;
}

export type ImportFailureReason =
  | 'tooLarge'
  | 'unsupported'
  | 'invalidJson'
  | 'validation'
  | 'busy'
  | 'unauthenticated'
  | 'forbidden'
  | 'sessionRequired'
  | 'gone'
  | 'network'
  | 'generic';

export interface ImportFailed {
  kind: 'failed';
  reason: ImportFailureReason;
  retryAfter: number | null;
  problems: FieldError[];
}

export interface ImportUpload {
  endpointId: string | null;
  keepTimes: boolean;
  format: ImportFormat;
  file: Blob;
  signal: AbortSignal;
  onProgress: (fraction: number) => void;
}

export class ImportNetworkError extends Error {
  constructor() {
    super('The import request did not complete.');
    this.name = 'ImportNetworkError';
  }
}

interface ParsedError {
  code: string | null;
  message: string | null;
  fields: FieldError[];
}

const IMPORT_RETRY_SECONDS = 60;
const SESSION_REFUSALS: readonly string[] = ['session_required', 'not_available_in_mode'];
const CONTENT_TYPES: Readonly<Record<ImportFormat, string>> = {
  json: 'application/json',
  ndjson: 'application/x-ndjson',
};
const ENDPOINT_LOCATION = /\/endpoints\/([^/]+)$/u;
const SECONDS_PATTERN = /^\d+$/u;

export function importPath(endpointId: string | null, keepTimes: boolean): string {
  const target = endpointId === null ? '/endpoints/import' : `/endpoints/${encodeURIComponent(endpointId)}/import`;
  return `${API_PREFIX}${target}?keep_times=${keepTimes ? 'true' : 'false'}`;
}

export function sendImport(upload: ImportUpload): Promise<ImportOutcome> {
  return new Promise<ImportOutcome>((resolve, reject) => {
    if (upload.signal.aborted) {
      reject(new ImportNetworkError());
      return;
    }
    const request = new XMLHttpRequest();
    request.open('POST', importPath(upload.endpointId, upload.keepTimes));
    request.setRequestHeader('Content-Type', CONTENT_TYPES[upload.format]);
    request.upload.onprogress = (event: ProgressEvent) => {
      if (event.lengthComputable && event.total > 0) {
        upload.onProgress(event.loaded / event.total);
      }
    };
    request.onload = () => {
      if (request.status >= 200 && request.status < 300) {
        resolve(outcomeOf(request.responseText, request.getResponseHeader('Location')));
        return;
      }
      reject(errorOf(request.status, request.responseText, request.getResponseHeader('Retry-After')));
    };
    request.onerror = () => {
      reject(new ImportNetworkError());
    };
    request.onabort = () => {
      reject(new ImportNetworkError());
    };
    upload.signal.addEventListener(
      'abort',
      () => {
        request.abort();
      },
      { once: true },
    );
    request.send(upload.file);
  });
}

export function failureOf(error: unknown): ImportFailed {
  if (error instanceof ImportNetworkError) {
    return failed('network', null, []);
  }
  if (!(error instanceof ApiError)) {
    return failed('generic', null, []);
  }
  switch (error.status) {
    case 400:
      return failed('invalidJson', null, []);
    case 401:
      return failed('unauthenticated', null, []);
    case 403:
      return failed(SESSION_REFUSALS.includes(error.code ?? '') ? 'sessionRequired' : 'forbidden', null, []);
    case 410:
      return failed('gone', null, []);
    case 413:
      return failed('tooLarge', null, []);
    case 415:
      return failed('unsupported', null, []);
    case 422:
      return failed('validation', null, error.details);
    case 429:
      return failed('busy', error.retryAfter ?? IMPORT_RETRY_SECONDS, []);
    default:
      return failed('generic', null, []);
  }
}

function failed(reason: ImportFailureReason, retryAfter: number | null, problems: FieldError[]): ImportFailed {
  return { kind: 'failed', reason, retryAfter, problems };
}

function outcomeOf(text: string, location: string | null): ImportOutcome {
  const body: UnknownRecord = parseRecord(text) ?? {};
  return {
    imported: countOf(body.imported),
    skipped: countOf(body.skipped),
    reason: reasonOf(body.reason),
    endpointId: textOf(body.endpoint_id) ?? endpointIdOf(location),
  };
}

function errorOf(status: number, text: string, retryAfter: string | null): ApiError {
  const parsed = parseErrorBody(text);
  return new ApiError({
    status,
    code: parsed.code,
    message: parsed.message ?? `Request failed with status ${status}`,
    details: parsed.fields,
    retryAfter: secondsOf(retryAfter),
  });
}

function parseErrorBody(text: string): ParsedError {
  const error = parseRecord(text)?.error;
  if (!isRecord(error)) {
    return { code: null, message: null, fields: [] };
  }
  return { code: textOf(error.code), message: textOf(error.message), fields: fieldsOf(error.fields) };
}

function fieldsOf(value: unknown): FieldError[] {
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

function endpointIdOf(location: string | null): string | null {
  if (location === null) {
    return null;
  }
  return ENDPOINT_LOCATION.exec(location)?.[1] ?? null;
}

function reasonOf(value: unknown): ImportReason | null {
  return value === 'capacity' || value === 'storage' ? value : null;
}

function countOf(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : null;
}

function textOf(value: unknown): string | null {
  return typeof value === 'string' && value !== '' ? value : null;
}

function secondsOf(value: string | null): number | null {
  return value !== null && SECONDS_PATTERN.test(value.trim()) ? Number(value.trim()) : null;
}
