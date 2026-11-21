import { describe, expect, it } from 'vitest';
import type { Instance } from '../api/types';
import { instanceFixture } from '../test/fixtures/instance';
import { requiresSignIn, safeReturnPath, signInHref } from './signIn';

const anonymousSso: Instance = { ...instanceFixture, auth_mode: 'sso', user: null };

describe('safeReturnPath', () => {
  it('keeps a path that starts with a single slash', () => {
    expect(safeReturnPath('/e/k7qm2xta9vnc')).toBe('/e/k7qm2xta9vnc');
  });

  it.each(['', 'endpoints', '//evil.example', '/\\evil.example', 42, undefined])('ignores %s', (value) => {
    expect(safeReturnPath(value)).toBeNull();
  });
});

describe('signInHref', () => {
  it('points at the auth route with the encoded return path', () => {
    expect(signInHref('/e/k7qm2xta9vnc?q=1')).toBe('/auth/login?return_to=%2Fe%2Fk7qm2xta9vnc%3Fq%3D1');
  });
});

describe('requiresSignIn', () => {
  it('requires sign-in for anonymous sso on every route except the sign-in route', () => {
    expect(requiresSignIn(anonymousSso, '/endpoints')).toBe(true);
    expect(requiresSignIn(anonymousSso, '/signin')).toBe(false);
  });

  it('never requires sign-in in open mode', () => {
    expect(requiresSignIn(instanceFixture, '/endpoints')).toBe(false);
  });
});
