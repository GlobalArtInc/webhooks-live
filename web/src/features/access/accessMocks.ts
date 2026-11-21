import { http } from 'msw';
import type { Endpoint } from '../../api/types';
import { endpointFixture, ENDPOINT_ID } from '../../test/fixtures/endpoint';
import { errorResponse, jsonResponse, readJsonBody } from '../../test/msw/responses';
import { mockServer } from '../../test/msw/server';
import { mockStore } from '../../test/msw/store';
import { instanceFixture } from '../../test/fixtures/instance';
import type { AccessPatch } from './types';

export const FAKE_UNLOCK_PASSWORD = 'not-a-real-password';
export const FAKE_GRANT = 'fake-grant-for-tests';
export const FAKE_NEW_PASSWORD = 'placeholder-pass-1';
export const FAKE_SECRET = 'placeholder-secret-1';
export const CLIENT_IP = '198.51.100.4';

export interface AccessMock {
  current: Endpoint;
  patches: AccessPatch[];
  unlockAttempts: string[];
}

export interface AccessMockOptions {
  unlockStatus?: number;
  patchStatus?: number;
  retryAfter?: string;
}

interface UnlockBody {
  password: string;
}

const ENDPOINT_URL = `/api/v1/endpoints/${ENDPOINT_ID}`;

export function endpointWith(fields: Record<string, unknown>): Endpoint {
  return Object.assign({}, endpointFixture, fields);
}

export function seedClientInstance(authMode: 'open' | 'sso' = 'open'): void {
  mockStore.instance = Object.assign({}, instanceFixture, { auth_mode: authMode, client_ip: CLIENT_IP });
}

function applyPatch(current: Endpoint, patch: AccessPatch): Endpoint {
  const next: Record<string, unknown> = {};
  if (patch.view_password !== undefined) {
    next.view_protected = patch.view_password !== null;
  }
  if (patch.sender_allow_cidrs !== undefined) {
    next.sender_allow_cidrs = patch.sender_allow_cidrs;
  }
  if (patch.capture_auth !== undefined) {
    next.capture_auth =
      patch.capture_auth === null
        ? null
        : patch.capture_auth.kind === 'basic'
          ? { kind: 'basic', username: patch.capture_auth.username }
          : { kind: 'header', name: patch.capture_auth.name };
  }
  return Object.assign({}, current, next);
}

export function installAccessMock(initial: Endpoint, options: AccessMockOptions = {}): AccessMock {
  const mock: AccessMock = { current: initial, patches: [], unlockAttempts: [] };
  mockServer.use(
    http.get(ENDPOINT_URL, () => jsonResponse(mock.current)),
    http.patch(ENDPOINT_URL, async ({ request }) => {
      const patch = await readJsonBody<AccessPatch>(request);
      if (patch === null) {
        return errorResponse(400, 'invalid_json', 'Request body is not valid JSON.');
      }
      mock.patches.push(patch);
      if (options.patchStatus !== undefined) {
        return errorResponse(options.patchStatus, 'internal_error', 'Unexpected failure.');
      }
      mock.current = applyPatch(mock.current, patch);
      return jsonResponse(mock.current);
    }),
    http.post(`${ENDPOINT_URL}/unlock`, async ({ request }) => {
      const body = await readJsonBody<UnlockBody>(request);
      mock.unlockAttempts.push(body?.password ?? '');
      if (options.unlockStatus === 429) {
        return new Response(JSON.stringify({ error: { code: 'rate_limited', message: 'Too many attempts.' } }), {
          status: 429,
          headers: { 'Content-Type': 'application/json', 'Retry-After': options.retryAfter ?? '30' },
        });
      }
      if (options.unlockStatus === 410) {
        return errorResponse(410, 'endpoint_gone', 'The endpoint is gone.');
      }
      if (body?.password === FAKE_UNLOCK_PASSWORD) {
        return jsonResponse({ grant: FAKE_GRANT, expires_at: new Date(Date.now() + 3_600_000).toISOString() });
      }
      return errorResponse(401, 'wrong_password', 'The password is wrong.');
    }),
  );
  return mock;
}
