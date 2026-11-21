import { describe, expect, it } from 'vitest';
import type { Endpoint, Folder } from '../../api/types';
import { endpointFixture } from '../../test/fixtures/endpoint';
import type { MyEndpoint } from '../endpoints/storage';
import { EMPTY_ORGANIZE, type LocalOrganize } from './localOrganize';
import {
  accountFolderKey,
  accountFolderViews,
  buildOrganized,
  filterOrganized,
  folderNameProblem,
  groupByFolder,
  labelCounts,
  localFolderKey,
  localFolderViews,
  type OrganizedEndpoint,
} from './model';

const ACCOUNT_ID = '0123456789abcdefghjkmnpq';
const LOCAL_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';
const ACCOUNT_FOLDER = '5a1b2c3d-0000-4000-8000-000000000001';
const LOCAL_FOLDER = 'local-receipts';

const accountEndpoint: Endpoint = {
  ...endpointFixture,
  id: ACCOUNT_ID,
  title: 'stripe-dev',
  tags: ['billing'],
  my_role: 'owner',
  last_active_at: '2026-11-21T12:00:00.000Z',
};

const localEntry: MyEndpoint = { id: LOCAL_ID, label: 'github-app', added: 1, lastOpened: 5, lastSeenCaptureId: null };

const accountFolder: Folder = {
  id: ACCOUNT_FOLDER,
  name: 'Payments',
  position: 0,
  endpoint_ids: [ACCOUNT_ID],
  created_at: '2026-11-21T10:00:00.000Z',
};

const placed: LocalOrganize = {
  folders: [{ id: LOCAL_FOLDER, name: 'Receipts' }],
  placements: [{ id: LOCAL_ID, folderId: LOCAL_FOLDER, tags: ['ci'] }],
};

describe('buildOrganized', () => {
  it('lists an endpoint known to the account once, with the account source', () => {
    const rows = buildOrganized({
      local: [localEntry, { ...localEntry, id: ACCOUNT_ID, label: null }],
      organize: EMPTY_ORGANIZE,
      account: [accountEndpoint],
      knownAccountIds: new Set([ACCOUNT_ID]),
      accountFolders: [accountFolder],
    });

    expect(rows.filter((row) => row.id === ACCOUNT_ID)).toEqual([
      expect.objectContaining({ source: 'account', label: 'stripe-dev', folderKey: accountFolderKey(ACCOUNT_FOLDER) }),
    ]);
    expect(rows.map((row) => row.source)).toEqual(['account', 'local']);
  });

  it('takes a local endpoint folder and labels from the browser store', () => {
    const [row] = buildOrganized({
      local: [localEntry],
      organize: placed,
      account: [],
      knownAccountIds: new Set(),
      accountFolders: [],
    });

    expect(row).toEqual(
      expect.objectContaining({ id: LOCAL_ID, source: 'local', folderKey: localFolderKey(LOCAL_FOLDER), tags: ['ci'] }),
    );
  });
});

describe('groupByFolder', () => {
  it('places unfiled endpoints last and omits the unfiled group when every endpoint has a folder', () => {
    const rows = buildOrganized({
      local: [localEntry],
      organize: placed,
      account: [accountEndpoint],
      knownAccountIds: new Set([ACCOUNT_ID]),
      accountFolders: [accountFolder],
    });
    const folders = [...accountFolderViews([accountFolder]), ...localFolderViews(placed.folders)];

    const groups = groupByFolder(rows, folders);

    expect(groups.map((group) => group.folder?.name ?? 'unfiled')).toEqual(['Payments', 'Receipts']);

    const unplaced = groupByFolder(
      buildOrganized({
        local: [localEntry],
        organize: EMPTY_ORGANIZE,
        account: [],
        knownAccountIds: new Set(),
        accountFolders: [],
      }),
      [],
    );
    expect(unplaced.map((group) => group.folder)).toEqual([null]);
  });
});

describe('labelCounts', () => {
  it('counts each merged endpoint once per label, most used first', () => {
    const rows: OrganizedEndpoint[] = [
      { ...buildRow(ACCOUNT_ID, 'account'), tags: ['billing', 'prod'] },
      { ...buildRow(LOCAL_ID, 'local'), tags: ['billing'] },
    ];

    expect(labelCounts(rows)).toEqual([
      { label: 'billing', count: 2 },
      { label: 'prod', count: 1 },
    ]);
  });
});

describe('folderNameProblem', () => {
  it('rejects an empty name, an over-long name and a duplicate ignoring case', () => {
    expect(folderNameProblem('   ', [])).toBe('empty');
    expect(folderNameProblem('x'.repeat(65), [])).toBe('tooLong');
    expect(folderNameProblem(' payments ', ['Payments'])).toBe('duplicate');
    expect(folderNameProblem('Refunds', ['Payments'])).toBeNull();
  });
});

describe('filterOrganized', () => {
  it('applies source, folder and label filters together', () => {
    const rows = buildOrganized({
      local: [localEntry],
      organize: placed,
      account: [accountEndpoint],
      knownAccountIds: new Set([ACCOUNT_ID]),
      accountFolders: [accountFolder],
    });

    expect(filterOrganized(rows, { source: 'local' }).map((row) => row.id)).toEqual([LOCAL_ID]);
    expect(filterOrganized(rows, { folder: 'unfiled' })).toEqual([]);
    expect(filterOrganized(rows, { label: 'billing' }).map((row) => row.id)).toEqual([ACCOUNT_ID]);
    expect(filterOrganized(rows, { folder: accountFolderKey(ACCOUNT_FOLDER), label: 'ci' })).toEqual([]);
  });
});

function buildRow(id: string, source: 'local' | 'account'): OrganizedEndpoint {
  return {
    id,
    source,
    role: null,
    label: null,
    folderKey: null,
    tags: [],
    captureCount: null,
    lastActivity: 0,
  };
}
