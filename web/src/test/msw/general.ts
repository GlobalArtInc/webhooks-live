import { http, type HttpHandler } from 'msw';
import { CLONE_PARTS, type Alias, type AliasReason, type ClonePart } from '../../api/general';
import type { Endpoint } from '../../api/types';
import { EXAMPLE_SECRET_NAME, usageFixture } from '../fixtures/general';
import { emptyResponse, errorResponse, jsonResponse, readJsonBody } from './responses';
import { mockStore, newEndpoint } from './store';

interface EndpointPath {
  endpointId: string;
}

interface AliasPath extends EndpointPath {
  alias: string;
}

interface AliasOnlyPath {
  alias: string;
}

interface CloneBody {
  title?: string;
  parts?: string[];
}

const MOCK_ALIAS_LIMIT = 5;
const ALIAS_PATTERN = /^[a-z0-9][a-z0-9-]{1,62}$/;
const RESERVED_ALIASES: readonly string[] = ['admin', 'api', 'static'];

const ALIASES = '/api/v1/endpoints/:endpointId/aliases';
const ALIAS = '/api/v1/endpoints/:endpointId/aliases/:alias';
const AVAILABILITY = '/api/v1/aliases/:alias/availability';
const USAGE = '/api/v1/usage';
const CLONE = '/api/v1/endpoints/:endpointId/clone';

function endpointMissing(): Response {
  return errorResponse(404, 'endpoint_not_found', 'Endpoint not found.');
}

function aliasesOf(endpointId: string): Alias[] {
  return mockStore.aliases.get(endpointId) ?? [];
}

function isClaimed(alias: string): boolean {
  return Array.from(mockStore.aliases.values()).some((list) => list.some((item) => item.alias === alias));
}

function reasonFor(alias: string): AliasReason | null {
  if (!ALIAS_PATTERN.test(alias)) {
    return 'invalid';
  }
  if (RESERVED_ALIASES.includes(alias)) {
    return 'reserved';
  }
  return isClaimed(alias) ? 'taken' : null;
}

function isClonePart(value: string): value is ClonePart {
  return CLONE_PARTS.some((part) => part === value);
}

function putAliasResponse(endpointId: string, alias: string): Response {
  if (!mockStore.endpoints.has(endpointId)) {
    return endpointMissing();
  }
  const reason = reasonFor(alias);
  if (reason === 'invalid') {
    return errorResponse(422, 'validation_failed', 'alias invalid_format', [
      { field: 'alias', code: 'invalid_format', message: 'use lowercase letters, digits and hyphens' },
    ]);
  }
  if (reason === 'reserved') {
    return errorResponse(422, 'validation_failed', 'alias reserved', [
      { field: 'alias', code: 'reserved', message: 'this alias is reserved' },
    ]);
  }
  if (reason === 'taken') {
    return errorResponse(409, 'alias_taken', 'This alias is already in use.');
  }
  const current = aliasesOf(endpointId);
  if (current.length >= MOCK_ALIAS_LIMIT) {
    return jsonResponse(
      {
        error: {
          code: 'limit_reached',
          message: 'Alias limit reached.',
          limit: { name: 'aliases', max: MOCK_ALIAS_LIMIT, window: null, resets_at: null },
        },
      },
      422,
    );
  }
  const created: Alias = {
    alias,
    url: `${mockStore.instance.public_url}/a/${alias}`,
    created_at: new Date().toISOString(),
  };
  mockStore.aliases.set(endpointId, [...current, created]);
  return jsonResponse(created);
}

function cloneEndpointResponse(endpointId: string, body: CloneBody | null): Response {
  const source = mockStore.endpoints.get(endpointId);
  if (source === undefined) {
    return endpointMissing();
  }
  const requested = body?.parts ?? [];
  const unknown = requested.find((part) => !isClonePart(part));
  if (unknown !== undefined) {
    return errorResponse(422, 'validation_failed', `parts ${unknown} is not known`, [
      { field: 'parts', code: 'invalid_format', message: `${unknown} is not a known part` },
    ]);
  }
  const copy: Endpoint = {
    ...newEndpoint({ title: body?.title ?? null }),
    cors: requested.includes('settings') ? source.cors : false,
  };
  mockStore.endpoints.set(copy.id, copy);
  const secretsMissing = requested.includes('verifier') ? [EXAMPLE_SECRET_NAME] : [];
  return jsonResponse({ ...copy, secrets_missing: secretsMissing }, 201);
}

export const generalHandlers: HttpHandler[] = [
  http.get<EndpointPath>(ALIASES, ({ params }) => {
    if (!mockStore.endpoints.has(params.endpointId)) {
      return endpointMissing();
    }
    return jsonResponse({ items: aliasesOf(params.endpointId) });
  }),

  http.put<AliasPath>(ALIAS, ({ params }) => putAliasResponse(params.endpointId, params.alias)),

  http.delete<AliasPath>(ALIAS, ({ params }) => {
    const current = aliasesOf(params.endpointId);
    if (!current.some((item) => item.alias === params.alias)) {
      return errorResponse(404, 'alias_not_found', 'Alias not found.');
    }
    mockStore.aliases.set(
      params.endpointId,
      current.filter((item) => item.alias !== params.alias),
    );
    return emptyResponse();
  }),

  http.get<AliasOnlyPath>(AVAILABILITY, ({ params }) => {
    const reason = reasonFor(params.alias);
    return jsonResponse({ alias: params.alias, available: reason === null, reason });
  }),

  http.get(USAGE, () => jsonResponse(usageFixture)),

  http.post<EndpointPath>(CLONE, async ({ params, request }) => {
    const body = await readJsonBody<CloneBody>(request, {});
    if (body === null) {
      return errorResponse(400, 'invalid_json', 'Request body is not valid JSON.');
    }
    return cloneEndpointResponse(params.endpointId, body);
  }),
];
