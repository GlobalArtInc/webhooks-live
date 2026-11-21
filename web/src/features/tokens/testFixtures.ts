import type { ApiToken, ApiTokenInput } from '../../api/tokens';
import type { Instance, User } from '../../api/types';
import { instanceFixture } from '../../test/fixtures/instance';

export const SIGNED_IN_USER: User = {
  id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21',
  email: 'owner@example.test',
  name: 'Owner',
};

export const SIGNED_IN_INSTANCE: Instance = { ...instanceFixture, auth_mode: 'mixed', user: SIGNED_IN_USER };

export const CREATED_TOKEN_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a99';

const SECRET_FILLER = 'FAKE';
const SECRET_FILLER_REPEATS = 13;

export function fakeSecret(): string {
  return ['whl', SECRET_FILLER.repeat(SECRET_FILLER_REPEATS)].join('_');
}

export function tokenFixture(overrides: Partial<ApiToken> = {}): ApiToken {
  return {
    id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a31',
    name: 'ci-runner',
    prefix: 'whl_FAKEFAKE',
    scopes: ['read', 'rules:write'],
    endpoint_ids: null,
    created_at: '2026-11-01T10:00:00.000Z',
    expires_at: '2099-01-01T00:00:00.000Z',
    last_used_at: null,
    revoked_at: null,
    ...overrides,
  };
}

export function createdTokenMetadata(input: ApiTokenInput): ApiToken {
  return tokenFixture({
    id: CREATED_TOKEN_ID,
    name: input.name,
    scopes: input.scopes,
    endpoint_ids: input.endpoint_ids ?? null,
    created_at: '2026-11-21T10:00:00.000Z',
  });
}
