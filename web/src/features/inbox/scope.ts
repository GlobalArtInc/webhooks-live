import { isEndpointId } from '../../app/ids';
import type { InboxCaptureQuery } from '../../api/types';
import { byRecentlyOpened, type MyEndpoint } from '../endpoints/storage';

export const SIGNED_IN_ENDPOINT_LIMIT = 50;
export const ANONYMOUS_ENDPOINT_LIMIT = 20;

const ALL = 'all';
const ENDPOINTS = 'endpoints';
const ENDPOINTS_PREFIX = 'endpoints:';
const FOLDER_PREFIX = 'folder:';
const FOLDER_ID_PATTERN = /^[A-Za-z0-9-]{1,64}$/;

export type InboxScope =
  { kind: 'all' } | { kind: 'folder'; folderId: string } | { kind: 'endpoints'; endpointIds: string[] };

export type SelectValue = 'all' | 'endpoints' | `folder:${string}`;

export function endpointLimit(signedIn: boolean): number {
  return signedIn ? SIGNED_IN_ENDPOINT_LIMIT : ANONYMOUS_ENDPOINT_LIMIT;
}

function validEndpointIds(raw: string): string[] {
  const ids = raw.split(',').filter(isEndpointId);
  return [...new Set(ids)];
}

export function parseInboxScope(value: string | undefined): InboxScope {
  if (value === undefined || value === ALL) {
    return { kind: 'all' };
  }
  if (value.startsWith(FOLDER_PREFIX)) {
    const folderId = value.slice(FOLDER_PREFIX.length);
    return FOLDER_ID_PATTERN.test(folderId) ? { kind: 'folder', folderId } : { kind: 'all' };
  }
  if (value === ENDPOINTS) {
    return { kind: 'endpoints', endpointIds: [] };
  }
  if (value.startsWith(ENDPOINTS_PREFIX)) {
    return { kind: 'endpoints', endpointIds: validEndpointIds(value.slice(ENDPOINTS_PREFIX.length)) };
  }
  return { kind: 'all' };
}

export function formatInboxScope(scope: InboxScope): string {
  switch (scope.kind) {
    case 'all':
      return ALL;
    case 'folder':
      return `${FOLDER_PREFIX}${scope.folderId}`;
    case 'endpoints':
      return scope.endpointIds.length === 0 ? ENDPOINTS : `${ENDPOINTS_PREFIX}${scope.endpointIds.join(',')}`;
  }
}

export function selectValueOf(scope: InboxScope): SelectValue {
  switch (scope.kind) {
    case 'all':
      return ALL;
    case 'folder':
      return `${FOLDER_PREFIX}${scope.folderId}`;
    case 'endpoints':
      return ENDPOINTS;
  }
}

export function resolveScope(scope: InboxScope, signedIn: boolean, local: readonly MyEndpoint[]): InboxScope {
  if (scope.kind === 'folder' && !signedIn) {
    return { kind: 'all' };
  }
  if (scope.kind === 'all' && !signedIn) {
    const recent = byRecentlyOpened(local).slice(0, ANONYMOUS_ENDPOINT_LIMIT);
    return { kind: 'endpoints', endpointIds: recent.map((entry) => entry.id) };
  }
  if (scope.kind === 'endpoints') {
    return { kind: 'endpoints', endpointIds: scope.endpointIds.slice(0, endpointLimit(signedIn)) };
  }
  return scope;
}

export function queryOf(scope: InboxScope): InboxCaptureQuery | null {
  switch (scope.kind) {
    case 'all':
      return {};
    case 'folder':
      return { folder: scope.folderId };
    case 'endpoints':
      return scope.endpointIds.length === 0 ? null : { endpoints: scope.endpointIds.join(',') };
  }
}

export function queryForKey(scopeKey: string): InboxCaptureQuery | null {
  return queryOf(parseInboxScope(scopeKey));
}
