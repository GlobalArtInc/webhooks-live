import { describe, expect, it } from 'vitest';
import {
  CURRENT_ORGANIZE_VERSION,
  EMPTY_ORGANIZE,
  parseOrganize,
  removeLocalFolder,
  serializeOrganize,
  tagLocalEndpoint,
  type LocalOrganize,
} from './localOrganize';

const FIRST_ID = '0123456789abcdefghjkmnpq';
const SECOND_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';

describe('parseOrganize', () => {
  it('keeps valid folders and placements and drops placements that name a missing folder', () => {
    const raw = JSON.stringify({
      version: CURRENT_ORGANIZE_VERSION,
      folders: [{ id: 'receipts', name: ' Receipts ' }],
      placements: [
        { id: FIRST_ID, folderId: 'receipts', tags: ['billing'] },
        { id: SECOND_ID, folderId: 'gone', tags: [] },
      ],
    });

    const state = parseOrganize(raw);

    expect(state.folders).toEqual([{ id: 'receipts', name: 'Receipts' }]);
    expect(state.placements).toEqual([
      { id: FIRST_ID, folderId: 'receipts', tags: ['billing'] },
      { id: SECOND_ID, folderId: null, tags: [] },
    ]);
  });

  it('returns an empty document for an unknown version or invalid JSON', () => {
    expect(parseOrganize('{not json')).toEqual(EMPTY_ORGANIZE);
    expect(parseOrganize(JSON.stringify({ version: 99, folders: [], placements: [] }))).toEqual(EMPTY_ORGANIZE);
  });
});

describe('local organize mutations', () => {
  it('round-trips through serialize and parse', () => {
    const state: LocalOrganize = tagLocalEndpoint(
      { folders: [{ id: 'receipts', name: 'Receipts' }], placements: [] },
      FIRST_ID,
      ['billing', 'prod'],
    );

    expect(parseOrganize(serializeOrganize(state))).toEqual(state);
  });

  it('moves the endpoints of a removed folder to unfiled', () => {
    const state: LocalOrganize = {
      folders: [{ id: 'receipts', name: 'Receipts' }],
      placements: [{ id: FIRST_ID, folderId: 'receipts', tags: [] }],
    };

    expect(removeLocalFolder(state, 'receipts')).toEqual({
      folders: [],
      placements: [{ id: FIRST_ID, folderId: null, tags: [] }],
    });
  });
});
