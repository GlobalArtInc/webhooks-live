import { describe, expect, it } from 'vitest';
import {
  addEndpoint,
  byRecentlyOpened,
  capEndpoints,
  MAX_LABEL_LENGTH,
  MAX_MY_ENDPOINTS,
  migrateEndpoints,
  type MyEndpoint,
  parseEndpoints,
  recordLastSeenCapture,
  removeEndpoint,
  renameEndpoint,
  serializeEndpoints,
  touchEndpoint,
  withEndpointTitle,
} from './storage';

const FIRST_ID = '0123456789abcdefghjkmnpq';
const SECOND_ID = 'k7m2q9x4w8c1d5f3h6j0n2p4';
const CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21';

function entry(id: string, lastOpened: number, label: string | null = null): MyEndpoint {
  return { id, label, added: lastOpened, lastOpened, lastSeenCaptureId: null };
}

function idFor(index: number): string {
  return `${String(index).padStart(6, '0')}abcdefghjkmnpqrstv`.slice(0, 24);
}

describe('my endpoints storage rules', () => {
  it('adds a new endpoint first with no label and touches an endpoint that is already present', () => {
    const added = addEndpoint([entry(FIRST_ID, 10)], SECOND_ID, 20);
    expect(added.map((item) => item.id)).toEqual([SECOND_ID, FIRST_ID]);
    expect(added[0]).toEqual({ id: SECOND_ID, label: null, added: 20, lastOpened: 20, lastSeenCaptureId: null });

    const touched = addEndpoint(added, FIRST_ID, 30);
    expect(touched).toHaveLength(2);
    expect(touched.find((item) => item.id === FIRST_ID)?.lastOpened).toBe(30);
  });

  it('caps the list at fifty and evicts the endpoint opened least recently, never the new one', () => {
    const full = Array.from({ length: MAX_MY_ENDPOINTS }, (_value, index) => entry(idFor(index), 1000 + index));
    const oldest = full[0];
    const capped = addEndpoint(full, SECOND_ID, 1000);

    expect(capped).toHaveLength(MAX_MY_ENDPOINTS);
    expect(capped.some((item) => item.id === oldest?.id)).toBe(false);
    expect(capped[0]?.id).toBe(SECOND_ID);
  });

  it('parses corrupt, unknown-version and malformed data as an empty list', () => {
    expect(parseEndpoints(null)).toEqual([]);
    expect(parseEndpoints('{not json')).toEqual([]);
    expect(parseEndpoints('[1, 2, 3]')).toEqual([]);
    expect(parseEndpoints(JSON.stringify({ version: 99, endpoints: [] }))).toEqual([]);
    expect(parseEndpoints(JSON.stringify({ version: 1, endpoints: 'nope' }))).toEqual([]);
  });

  it('migrates version 1 data, dropping invalid and duplicate entries', () => {
    const migrated = migrateEndpoints({
      version: 1,
      endpoints: [
        { id: FIRST_ID, label: '  stripe  ', added: 1, lastOpened: 2, lastSeenCaptureId: CAPTURE_ID },
        { id: FIRST_ID, label: 'duplicate', added: 3, lastOpened: 4, lastSeenCaptureId: null },
        { id: 'not-an-endpoint-id', label: null, added: 1, lastOpened: 1, lastSeenCaptureId: null },
        { id: SECOND_ID, label: 7, added: 'soon', lastOpened: 1, lastSeenCaptureId: null },
      ],
    });

    expect(migrated).toEqual([
      { id: FIRST_ID, label: 'stripe', added: 1, lastOpened: 2, lastSeenCaptureId: CAPTURE_ID },
    ]);
  });

  it('round-trips through the versioned serialised form', () => {
    const list = [entry(FIRST_ID, 5, 'github-app')];
    expect(JSON.parse(serializeEndpoints(list))).toEqual({ version: 1, endpoints: list });
    expect(parseEndpoints(serializeEndpoints(list))).toEqual(list);
  });

  it('renames with trimmed labels, clears empty labels and clamps long ones', () => {
    const list = [entry(FIRST_ID, 1)];
    expect(renameEndpoint(list, FIRST_ID, '  shop  ')[0]?.label).toBe('shop');
    expect(renameEndpoint(list, FIRST_ID, '   ')[0]?.label).toBeNull();
    expect(renameEndpoint(list, FIRST_ID, 'x'.repeat(MAX_LABEL_LENGTH + 5))[0]?.label).toHaveLength(MAX_LABEL_LENGTH);
  });

  it('touches, records the last seen capture, removes, and orders by last opened', () => {
    const list = [entry(FIRST_ID, 1), entry(SECOND_ID, 2)];

    expect(touchEndpoint(list, FIRST_ID, 9).map((item) => item.lastOpened)).toEqual([9, 2]);
    expect(recordLastSeenCapture(list, SECOND_ID, CAPTURE_ID)[1]?.lastSeenCaptureId).toBe(CAPTURE_ID);
    expect(removeEndpoint(list, FIRST_ID).map((item) => item.id)).toEqual([SECOND_ID]);
    expect(byRecentlyOpened(list).map((item) => item.id)).toEqual([SECOND_ID, FIRST_ID]);
  });

  it('keeps the newest entries when capping data that is already over the limit', () => {
    const over = Array.from({ length: MAX_MY_ENDPOINTS + 2 }, (_value, index) => entry(idFor(index), index));
    const capped = capEndpoints(over);

    expect(capped).toHaveLength(MAX_MY_ENDPOINTS);
    expect(capped[0]?.id).toBe(idFor(2));
  });
});

describe('known endpoint titles in the stored record', () => {
  it('loads an old record without a title and keeps a stored title when present', () => {
    const migrated = migrateEndpoints({
      version: 1,
      endpoints: [
        { id: FIRST_ID, label: null, added: 1, lastOpened: 2, lastSeenCaptureId: null },
        { id: SECOND_ID, label: null, title: '  Stripe test  ', added: 1, lastOpened: 2, lastSeenCaptureId: null },
      ],
    });

    expect(migrated[0]).not.toHaveProperty('title');
    expect(migrated[1]?.title).toBe('Stripe test');
    expect(parseEndpoints(serializeEndpoints(migrated))).toEqual(migrated);
  });

  it('merges a title into the existing record without adding or duplicating entries', () => {
    const list = [entry(FIRST_ID, 10), entry(SECOND_ID, 5)];

    const titled = withEndpointTitle(list, SECOND_ID, 'Stripe test');
    expect(titled.map((item) => item.id)).toEqual([FIRST_ID, SECOND_ID]);
    expect(titled[1]?.title).toBe('Stripe test');
    expect(titled[0]).toBe(list[0]);
    expect(withEndpointTitle(titled, SECOND_ID, 'Stripe test')[1]).toBe(titled[1]);
    expect(withEndpointTitle(list, 'zzzzzzzzzzzzzzzzzzzzzzzz', 'Other')).toEqual(list);
    expect(withEndpointTitle(titled, SECOND_ID, null)[1]).not.toHaveProperty('title');
  });

  it('stores the title of a new endpoint and fills it in for one that is already present', () => {
    const created = addEndpoint([], FIRST_ID, 10, 'Stripe test');
    expect(created[0]?.title).toBe('Stripe test');

    const again = addEndpoint([entry(FIRST_ID, 5)], FIRST_ID, 20, 'Renamed');
    expect(again).toHaveLength(1);
    expect(again[0]).toMatchObject({ lastOpened: 20, title: 'Renamed' });
  });

  it('limits the stored title to the label length', () => {
    const [titled] = withEndpointTitle([entry(FIRST_ID, 1)], FIRST_ID, 'T'.repeat(MAX_LABEL_LENGTH + 20));
    expect(Array.from(titled?.title ?? '')).toHaveLength(MAX_LABEL_LENGTH);
  });
});
