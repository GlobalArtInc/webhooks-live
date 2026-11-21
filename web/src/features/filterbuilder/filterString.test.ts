import { describe, expect, it } from 'vitest';
import { filterStringOf, normalisedSearch, patchFor, searchOfFilterString } from './filterString';

describe('filter strings', () => {
  it('writes the set parameters in a fixed order and omits the absolute start when a range is set', () => {
    expect(
      filterStringOf({
        path: '/orders',
        method: 'POST',
        range: '1h',
        since: '2026-11-21T13:00:00.000Z',
        q: 'invoice_id',
      }),
    ).toBe('q=invoice_id&method=POST&range=1h&path=%2Forders');
  });

  it('reopens a saved string with a fresh start for its relative range', () => {
    const now = Date.parse('2026-11-21T14:00:00Z');
    expect(searchOfFilterString('kind=email%2Cdns&range=15m&ip=203.0.113.7', now)).toEqual({
      kind: 'email,dns',
      range: '15m',
      since: '2026-11-21T13:45:00.000Z',
      ip: '203.0.113.7',
    });
  });

  it('turns a draft into the search the URL would hold, dropping what does not validate', () => {
    expect(normalisedSearch({ method: 'GET,PUT', status: '9xx', path: 'hooks', q: '  ' }, 0)).toEqual({
      method: 'GET,PUT',
      path: '/hooks',
    });
  });

  it('builds a patch that overrides every parameter the search does not hold', () => {
    const merged = { ...{ method: 'GET', kind: 'dns' }, ...patchFor({ kind: 'http' }) };
    expect(merged).toEqual({ kind: 'http' });
    expect(merged).not.toHaveProperty('method', 'GET');
  });
});
