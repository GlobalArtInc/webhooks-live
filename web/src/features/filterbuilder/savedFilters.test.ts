import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  addSavedFilter,
  deleteSavedFilter,
  loadSavedFilters,
  MAX_SAVED_FILTERS,
  parseSavedFilters,
  renameSavedFilter,
  storeSavedFilters,
  type SavedFilter,
} from './savedFilters';

const ENDPOINT_ID = '0123456789abcdefghjkmnpq';

function entry(index: number): SavedFilter {
  return { id: `id-${index}`, name: `Filter ${index}`, filter: 'method=POST' };
}

afterEach(() => {
  window.localStorage.clear();
  vi.restoreAllMocks();
});

describe('parseSavedFilters', () => {
  it('reads nothing from missing, broken or foreign storage and drops entries without a name', () => {
    expect(parseSavedFilters(null)).toEqual([]);
    expect(parseSavedFilters('{not json')).toEqual([]);
    expect(parseSavedFilters('{"id":"x"}')).toEqual([]);
    const raw = JSON.stringify([{ id: 'a', name: '   ', filter: 'q=x' }, { id: 'b', name: 'Kept', filter: 'q=x' }, 7]);
    expect(parseSavedFilters(raw)).toEqual([{ id: 'b', name: 'Kept', filter: 'q=x' }]);
  });

  it('keeps at most the maximum number of filters', () => {
    const raw = JSON.stringify(Array.from({ length: MAX_SAVED_FILTERS + 5 }, (_value, index) => entry(index)));
    expect(parseSavedFilters(raw)).toHaveLength(MAX_SAVED_FILTERS);
  });
});

describe('saved filter store', () => {
  it('stores each endpoint separately and reads back what it wrote', () => {
    expect(storeSavedFilters(ENDPOINT_ID, [entry(1)])).toBe(true);
    expect(loadSavedFilters(ENDPOINT_ID)).toEqual([entry(1)]);
    expect(loadSavedFilters('zzzzzzzzzzzzzzzzzzzzzzzz')).toEqual([]);
  });

  it('reports a failed write instead of throwing', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota exceeded');
    });
    expect(storeSavedFilters(ENDPOINT_ID, [entry(1)])).toBe(false);
  });

  it('reads nothing when the storage itself refuses access', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('access denied');
    });
    expect(loadSavedFilters(ENDPOINT_ID)).toEqual([]);
  });
});

describe('saved filter list operations', () => {
  it('adds up to the maximum, renames by id and deletes by id', () => {
    const full = Array.from({ length: MAX_SAVED_FILTERS }, (_value, index) => entry(index));
    expect(addSavedFilter(full, entry(99))).toBe(full);
    expect(addSavedFilter([entry(1)], entry(2))).toEqual([entry(1), entry(2)]);
    expect(renameSavedFilter([entry(1)], 'id-1', 'Renamed')).toEqual([{ ...entry(1), name: 'Renamed' }]);
    expect(deleteSavedFilter([entry(1), entry(2)], 'id-1')).toEqual([entry(2)]);
  });
});
