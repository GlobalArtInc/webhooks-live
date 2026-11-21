import { describe, expect, it } from 'vitest';
import {
  activePartCount,
  hasCaptureSearch,
  listFiltersOf,
  rangeSince,
  toggleToken,
  validateCaptureSearch,
} from './captureSearch';

describe('validateCaptureSearch', () => {
  it('keeps known methods and status classes in upper or lower case and drops the rest', () => {
    expect(validateCaptureSearch({ method: 'post,put,BREW', status: '4XX,2xx,9xx,none' })).toEqual({
      method: 'POST,PUT',
      status: '4xx,2xx,none',
    });
  });

  it('keeps known capture kinds in any case and drops the rest', () => {
    expect(validateCaptureSearch({ kind: 'EMAIL,dns,schedule,mail' })).toEqual({ kind: 'email,dns' });
    expect(validateCaptureSearch({ kind: 'schedule' })).toEqual({});
  });

  it('trims and caps the text query and drops an empty one', () => {
    expect(validateCaptureSearch({ q: '  invoice  ' })).toEqual({ q: 'invoice' });
    expect(validateCaptureSearch({ q: '   ' })).toEqual({});
    expect(validateCaptureSearch({ q: 'x'.repeat(250) })).toEqual({ q: 'x'.repeat(200) });
    expect(validateCaptureSearch({ q: 42 })).toEqual({ q: '42' });
  });

  it('keeps a time range only together with a valid start instant', () => {
    expect(validateCaptureSearch({ range: '1h', since: '2026-11-21T13:00:00Z' })).toEqual({
      range: '1h',
      since: '2026-11-21T13:00:00.000Z',
    });
    expect(validateCaptureSearch({ range: '1h' })).toEqual({});
    expect(validateCaptureSearch({ range: '2h', since: '2026-11-21T13:00:00Z' })).toEqual({
      since: '2026-11-21T13:00:00.000Z',
    });
    expect(validateCaptureSearch({ since: 'yesterday' })).toEqual({});
  });
});

describe('listFiltersOf and helpers', () => {
  it('maps the search onto the list query without the time range label', () => {
    const search = validateCaptureSearch({
      method: 'POST',
      kind: 'email',
      status: '5xx',
      q: 'x',
      range: '7d',
      since: '2026-11-14T00:00:00Z',
    });

    expect(listFiltersOf(search)).toEqual({
      method: 'POST',
      kind: 'email',
      status: '5xx',
      q: 'x',
      since: '2026-11-14T00:00:00.000Z',
    });
  });

  it('adds and removes one token from a comma list and clears an empty list', () => {
    expect(toggleToken(undefined, 'GET')).toBe('GET');
    expect(toggleToken('GET', 'POST')).toBe('GET,POST');
    expect(toggleToken('GET,POST', 'GET')).toBe('POST');
    expect(toggleToken('GET', 'GET')).toBeUndefined();
  });

  it('counts the active chips and the range as filter parts', () => {
    expect(activePartCount({ method: 'GET,POST', status: '2xx', range: '1h', since: '2026-11-21T13:00:00Z' })).toBe(4);
    expect(activePartCount({ kind: 'email,dns' })).toBe(2);
    expect(activePartCount({ q: 'only text' })).toBe(0);
  });

  it('computes the start of a range from the given instant', () => {
    expect(rangeSince('15m', Date.parse('2026-11-21T14:00:00Z'))).toBe('2026-11-21T13:45:00.000Z');
  });
});

describe('sender and path parameters', () => {
  it('keeps an IPv4 address or range and an exact IPv6 address in lower case', () => {
    expect(validateCaptureSearch({ ip: '203.0.113.0/24' })).toEqual({ ip: '203.0.113.0/24' });
    expect(validateCaptureSearch({ ip: '2001:DB8::7' })).toEqual({ ip: '2001:db8::7' });
  });

  it('drops a malformed sender, an IPv6 range and an IPv4 range wider than 32 bits', () => {
    expect(validateCaptureSearch({ ip: '203.0.113' })).toEqual({});
    expect(validateCaptureSearch({ ip: '203.0.113.0/33' })).toEqual({});
    expect(validateCaptureSearch({ ip: '2001:db8::/32' })).toEqual({});
  });

  it('keeps the path prefix with a leading slash and drops a blank one', () => {
    expect(validateCaptureSearch({ path: 'orders' })).toEqual({ path: '/orders' });
    expect(validateCaptureSearch({ path: '/hooks/stripe' })).toEqual({ path: '/hooks/stripe' });
    expect(validateCaptureSearch({ path: '  ' })).toEqual({});
  });

  it('counts the sender and the path as filter parts', () => {
    expect(activePartCount({ ip: '203.0.113.7', path: '/orders' })).toBe(2);
    expect(hasCaptureSearch({ path: '/orders' })).toBe(true);
  });
});

describe('starred parameter', () => {
  it('keeps true and false in either form and drops anything else', () => {
    expect(validateCaptureSearch({ starred: 'true' })).toEqual({ starred: true });
    expect(validateCaptureSearch({ starred: false })).toEqual({ starred: false });
    expect(validateCaptureSearch({ starred: 'yes' })).toEqual({});
  });

  it('maps the flag onto the list query, including the false value', () => {
    expect(listFiltersOf(validateCaptureSearch({ starred: 'false' }))).toEqual({ starred: false });
    expect(activePartCount({ starred: true })).toBe(1);
  });
});
