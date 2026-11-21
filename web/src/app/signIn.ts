import type { Instance } from '../api/types';

export const SIGN_IN_ROUTE = '/signin';
const SIGN_IN_PATH = '/auth/login';

export function safeReturnPath(value: unknown): string | null {
  if (typeof value !== 'string' || !value.startsWith('/') || value.startsWith('//') || value.includes('\\')) {
    return null;
  }
  return value;
}

export function signInHref(returnTo: string): string {
  return `${SIGN_IN_PATH}?${new URLSearchParams({ return_to: returnTo }).toString()}`;
}

export function isAnonymousSso(instance: Instance): boolean {
  return instance.auth_mode === 'sso' && instance.user === null;
}

export function requiresSignIn(instance: Instance, pathname: string): boolean {
  return isAnonymousSso(instance) && pathname !== SIGN_IN_ROUTE;
}
