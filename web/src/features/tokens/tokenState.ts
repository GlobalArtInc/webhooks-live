import type { ApiToken } from '../../api/tokens';

export type TokenState = 'active' | 'expired' | 'revoked';

export function tokenState(token: ApiToken, now: number): TokenState {
  if (token.revoked_at !== null) {
    return 'revoked';
  }
  return Date.parse(token.expires_at) <= now ? 'expired' : 'active';
}
