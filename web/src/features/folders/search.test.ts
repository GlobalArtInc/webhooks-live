import { describe, expect, it } from 'vitest';
import { serverQueryFor, validateEndpointsSearch } from './search';

const ACCOUNT_FOLDER = '5a1b2c3d-0000-4000-8000-000000000001';

describe('validateEndpointsSearch', () => {
  it('keeps known filter values and drops the rest', () => {
    expect(validateEndpointsSearch({ source: 'other', label: 'Bad Label', folder: 'local:receipts' })).toEqual({
      folder: 'local:receipts',
    });
    expect(validateEndpointsSearch({ source: 'shared', label: 'billing', folder: 'unfiled' })).toEqual({
      source: 'shared',
      label: 'billing',
      folder: 'unfiled',
    });
  });
});

describe('serverQueryFor', () => {
  it('sends the tag and the account folder to the server and keeps local folders on the client', () => {
    expect(serverQueryFor({ label: 'billing', folder: `account:${ACCOUNT_FOLDER}` })).toEqual({
      tag: 'billing',
      folder: ACCOUNT_FOLDER,
    });
    expect(serverQueryFor({ folder: 'local:receipts' })).toEqual({ tag: undefined, folder: undefined });
  });
});
