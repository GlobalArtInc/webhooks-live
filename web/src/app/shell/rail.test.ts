import { describe, expect, it } from 'vitest';
import { showsEndpointRail } from './rail';

describe('showsEndpointRail', () => {
  it('shows the rail on home, my endpoints and every endpoint route', () => {
    expect(showsEndpointRail('/')).toBe(true);
    expect(showsEndpointRail('/endpoints')).toBe(true);
    expect(showsEndpointRail('/e/0123456789abcdefghjkmnpq')).toBe(true);
    expect(showsEndpointRail('/e/0123456789abcdefghjkmnpq/settings')).toBe(true);
  });

  it('hides the rail on global settings, sign-in and unknown paths', () => {
    expect(showsEndpointRail('/settings')).toBe(false);
    expect(showsEndpointRail('/signin')).toBe(false);
    expect(showsEndpointRail('/no/such/page')).toBe(false);
  });
});
