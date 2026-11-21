import type { EndpointInput, FieldError } from '../../api/types';

export const STATUS_MIN = 100;
export const STATUS_MAX = 599;
export const DELAY_MAX_MS = 10000;
export const MAX_RESPONSE_HEADERS = 20;
export const HEADER_NAME_MAX_BYTES = 64;
export const HEADER_VALUE_MAX_BYTES = 1024;
export const CONTENT_TYPE_MAX_BYTES = 255;
export const BODY_MAX_BYTES = 65536;

export const FIELD_PATHS = {
  status: 'response.status',
  contentType: 'response.content_type',
  body: 'response.body',
  delay: 'response.delay_ms',
  headers: 'response.headers',
  cors: 'cors',
} as const;

export type HeaderPart = 'name' | 'value';
export type IssueCode = 'required' | 'invalid' | 'tooLong' | 'outOfRange' | 'duplicate' | 'tooMany' | 'notAllowed';

export interface FieldIssue {
  code: IssueCode;
  limit: number | null;
}

export type IssueMap = ReadonlyMap<string, FieldIssue>;

export interface HeaderRow {
  key: number;
  name: string;
  value: string;
}

export interface ResponseDraft {
  status: string;
  contentType: string;
  body: string;
  delay: string;
  cors: boolean;
  headers: readonly HeaderRow[];
}

const TOKEN_PATTERN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const VISIBLE_VALUE_PATTERN = /^[ -~\t]*$/;
const INTEGER_PATTERN = /^\d+$/;
const HEADER_NAME_PATH = /^response\.headers\.\d+\.name$/;
const HEADER_VALUE_PATH = /^response\.headers\.\d+\.value$/;
const encoder = new TextEncoder();
let rowKeySeed = 0;

export function byteLength(text: string): number {
  return encoder.encode(text).length;
}

export function createHeaderRow(): HeaderRow {
  rowKeySeed += 1;
  return { key: rowKeySeed, name: '', value: '' };
}

export function createDefaultDraft(): ResponseDraft {
  return { status: '200', contentType: 'text/plain', body: '', delay: '0', cors: false, headers: [] };
}

export function headerFieldPath(index: number, part: HeaderPart): string {
  return `${FIELD_PATHS.headers}.${index}.${part}`;
}

function issue(code: IssueCode, limit: number | null = null): FieldIssue {
  return { code, limit };
}

function checkStatus(text: string, issues: Map<string, FieldIssue>): void {
  const value = text.trim();
  if (value === '') {
    issues.set(FIELD_PATHS.status, issue('required'));
  } else if (!INTEGER_PATTERN.test(value)) {
    issues.set(FIELD_PATHS.status, issue('invalid'));
  } else if (Number(value) < STATUS_MIN || Number(value) > STATUS_MAX) {
    issues.set(FIELD_PATHS.status, issue('outOfRange'));
  }
}

function checkContentType(text: string, issues: Map<string, FieldIssue>): void {
  const value = text.trim();
  if (value === '') {
    issues.set(FIELD_PATHS.contentType, issue('required'));
  } else if (byteLength(value) > CONTENT_TYPE_MAX_BYTES) {
    issues.set(FIELD_PATHS.contentType, issue('tooLong', CONTENT_TYPE_MAX_BYTES));
  }
}

function checkBody(text: string, issues: Map<string, FieldIssue>): void {
  if (byteLength(text) > BODY_MAX_BYTES) {
    issues.set(FIELD_PATHS.body, issue('tooLong', BODY_MAX_BYTES));
  }
}

function checkDelay(text: string, issues: Map<string, FieldIssue>): void {
  const value = text.trim();
  if (value === '') {
    issues.set(FIELD_PATHS.delay, issue('required'));
  } else if (!INTEGER_PATTERN.test(value)) {
    issues.set(FIELD_PATHS.delay, issue('invalid'));
  } else if (Number(value) > DELAY_MAX_MS) {
    issues.set(FIELD_PATHS.delay, issue('outOfRange'));
  }
}

function checkNameText(name: string, path: string, seen: Set<string>, issues: Map<string, FieldIssue>): void {
  if (!TOKEN_PATTERN.test(name)) {
    issues.set(path, issue('invalid'));
  } else if (byteLength(name) > HEADER_NAME_MAX_BYTES) {
    issues.set(path, issue('tooLong', HEADER_NAME_MAX_BYTES));
  } else if (seen.has(name.toLowerCase())) {
    issues.set(path, issue('duplicate'));
  } else {
    seen.add(name.toLowerCase());
  }
}

function checkHeaderRow(row: HeaderRow, index: number, seen: Set<string>, issues: Map<string, FieldIssue>): void {
  const name = row.name.trim();
  const value = row.value.trim();
  const namePath = headerFieldPath(index, 'name');
  const valuePath = headerFieldPath(index, 'value');
  if (name === '') {
    issues.set(namePath, issue('required'));
  } else {
    checkNameText(name, namePath, seen, issues);
  }
  if (!VISIBLE_VALUE_PATTERN.test(value)) {
    issues.set(valuePath, issue('invalid'));
  } else if (byteLength(value) > HEADER_VALUE_MAX_BYTES) {
    issues.set(valuePath, issue('tooLong', HEADER_VALUE_MAX_BYTES));
  }
}

function checkHeaders(rows: readonly HeaderRow[], issues: Map<string, FieldIssue>): void {
  if (rows.length > MAX_RESPONSE_HEADERS) {
    issues.set(FIELD_PATHS.headers, issue('tooMany', MAX_RESPONSE_HEADERS));
  }
  const seen = new Set<string>();
  rows.forEach((row, index) => checkHeaderRow(row, index, seen, issues));
}

export function validateDraft(draft: ResponseDraft): IssueMap {
  const issues = new Map<string, FieldIssue>();
  checkStatus(draft.status, issues);
  checkContentType(draft.contentType, issues);
  checkBody(draft.body, issues);
  checkDelay(draft.delay, issues);
  checkHeaders(draft.headers, issues);
  return issues;
}

export function buildEndpointInput(draft: ResponseDraft): EndpointInput {
  return {
    cors: draft.cors,
    response: {
      status: Number(draft.status.trim()),
      content_type: draft.contentType.trim(),
      body: { encoding: 'utf8', data: draft.body },
      delay_ms: Number(draft.delay.trim()),
      headers: draft.headers.map((row) => ({ name: row.name.trim(), value: row.value.trim() })),
    },
  };
}

function normalizeServerPath(path: string): string {
  return path === FIELD_PATHS.body || path.startsWith(`${FIELD_PATHS.body}.`) ? FIELD_PATHS.body : path;
}

function limitForPath(path: string): number | null {
  if (path === FIELD_PATHS.contentType) {
    return CONTENT_TYPE_MAX_BYTES;
  }
  if (path === FIELD_PATHS.body) {
    return BODY_MAX_BYTES;
  }
  if (path === FIELD_PATHS.headers) {
    return MAX_RESPONSE_HEADERS;
  }
  if (HEADER_NAME_PATH.test(path)) {
    return HEADER_NAME_MAX_BYTES;
  }
  if (HEADER_VALUE_PATH.test(path)) {
    return HEADER_VALUE_MAX_BYTES;
  }
  return null;
}

function issueCodeForServer(code: string | undefined): IssueCode {
  switch (code) {
    case 'required':
      return 'required';
    case 'too_long':
      return 'tooLong';
    case 'out_of_range':
      return 'outOfRange';
    case 'too_many':
      return 'tooMany';
    case 'duplicate':
      return 'duplicate';
    case 'not_allowed':
      return 'notAllowed';
    default:
      return 'invalid';
  }
}

export function issuesFromServer(fields: readonly FieldError[]): IssueMap {
  const issues = new Map<string, FieldIssue>();
  for (const field of fields) {
    const path = normalizeServerPath(field.field);
    issues.set(path, issue(issueCodeForServer(field.code), limitForPath(path)));
  }
  return issues;
}

export const NO_ISSUES: IssueMap = new Map();
