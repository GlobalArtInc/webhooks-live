import { delay, http, type HttpHandler } from 'msw';
import type { ApiToken, ApiTokenInput } from '../../api/tokens';
import { emptyResponse, errorResponse, jsonResponse, readJsonBody } from '../../test/msw/responses';
import { createdTokenMetadata, fakeSecret } from './testFixtures';

export interface TokenStore {
  tokens: ApiToken[];
  created: ApiTokenInput[];
  revoked: string[];
  createFailure: Response | null;
  commitThenFail: boolean;
  hang: boolean;
}

const TOKENS = '/api/v1/tokens';
const TOKEN = '/api/v1/tokens/:tokenId';
const MAX_NAME_CHARS = 64;

export function newTokenStore(tokens: ApiToken[] = []): TokenStore {
  return { tokens, created: [], revoked: [], createFailure: null, commitThenFail: false, hang: false };
}

export function tokenHandlers(store: TokenStore): HttpHandler[] {
  return [
    http.get(TOKENS, () => jsonResponse({ items: store.tokens })),
    http.post(TOKENS, async ({ request }) => {
      if (store.hang) {
        await delay('infinite');
      }
      if (store.createFailure !== null) {
        return store.createFailure;
      }
      const input = await readJsonBody<ApiTokenInput>(request);
      if (input === null || input.name.length === 0 || input.name.length > MAX_NAME_CHARS) {
        return errorResponse(422, 'validation_failed', 'name is invalid', [{ field: 'name', message: 'is invalid' }]);
      }
      store.created.push(input);
      const metadata = createdTokenMetadata(input);
      store.tokens = [metadata, ...store.tokens];
      if (store.commitThenFail) {
        return errorResponse(502, 'bad_gateway', 'The answer was lost.');
      }
      return jsonResponse({ ...metadata, token: fakeSecret() }, 201);
    }),
    http.delete<{ tokenId: string }>(TOKEN, ({ params }) => {
      const existing = store.tokens.find((token) => token.id === params.tokenId);
      if (existing === undefined) {
        return errorResponse(404, 'token_not_found', 'Token not found.');
      }
      store.revoked.push(existing.id);
      store.tokens = store.tokens.map((token) =>
        token.id === existing.id ? { ...token, revoked_at: '2026-11-21T10:30:00.000Z' } : token,
      );
      return emptyResponse();
    }),
  ];
}

export function limitFailure(): Response {
  return jsonResponse({ error: { code: 'limit_reached', message: 'limit', limit: { name: 'tokens', max: 20 } } }, 422);
}

export function rateFailure(resetsAt: string): Response {
  return jsonResponse(
    {
      error: {
        code: 'quota_exceeded',
        message: 'slow down',
        limit: { name: 'token_creation', window: 'hour', max: 10, resets_at: resetsAt },
      },
    },
    429,
    { 'Retry-After': '900' },
  );
}
