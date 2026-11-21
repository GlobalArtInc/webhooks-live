import { describe, expect, it } from 'vitest';
import { activePartCount, listFiltersOf, rangeSince, toggleToken, validateCaptureSearch } from './captureSearch';

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
