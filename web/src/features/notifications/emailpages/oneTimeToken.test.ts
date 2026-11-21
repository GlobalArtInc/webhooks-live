import { afterEach, describe, expect, it } from 'vitest';
import { stripTokenFromUrl, tokenFromSearch } from './oneTimeToken';

afterEach(() => {
  window.history.replaceState(null, '', '/');
});

describe('one-time token from the address bar', () => {
  it('reads the token parameter and ignores an empty one', () => {
    expect(tokenFromSearch('?token=test-only-verify-token')).toBe('test-only-verify-token');
    expect(tokenFromSearch('?token=')).toBeNull();
    expect(tokenFromSearch('')).toBeNull();
  });

  it('removes only the token from the address bar and keeps other parameters', () => {
    window.history.replaceState(null, '', '/email/verify?token=test-only-verify-token&lang=en');
    stripTokenFromUrl();
    expect(window.location.search).toBe('?lang=en');
    expect(window.location.href).not.toContain('test-only-verify-token');
  });
});
