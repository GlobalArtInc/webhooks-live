import { describe, expect, it } from 'vitest';
import { collapsesRail, fillsViewport, showsEndpointRail } from './rail';

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

describe('collapsesRail', () => {
  it('collapses the rail on the response section and the rule editor only', () => {
    expect(collapsesRail('/e/0123456789abcdefghjkmnpq/settings/response')).toBe(true);
    expect(collapsesRail('/e/0123456789abcdefghjkmnpq/settings/rules/0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21')).toBe(true);
    expect(collapsesRail('/e/0123456789abcdefghjkmnpq/settings/rules')).toBe(false);
    expect(collapsesRail('/e/0123456789abcdefghjkmnpq/settings/general')).toBe(false);
    expect(collapsesRail('/e/0123456789abcdefghjkmnpq')).toBe(false);
  });
});

describe('fillsViewport', () => {
  it('fills the window on the capture list, a selected capture and the workspace only', () => {
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq')).toBe(true);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/')).toBe(true);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/c/0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21')).toBe(true);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/settings')).toBe(true);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/settings/general')).toBe(true);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/settings/rules/0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21')).toBe(true);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/settingsx')).toBe(false);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/unlock')).toBe(false);
    expect(fillsViewport('/e/0123456789abcdefghjkmnpq/diff/a/b')).toBe(false);
    expect(fillsViewport('/endpoints')).toBe(false);
  });
});
